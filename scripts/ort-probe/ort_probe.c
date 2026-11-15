// ort_probe: on-device CPU benchmark for OHOS using the repo's ONNX Runtime build.
//
// Usage:  ort_probe <model.onnx> <iters> <threads> <in0.bin> [in1.bin ...]
//   - Inputs are raw little-endian tensors; the tensor shape/type is read from the model.
//   - Dynamic dims can be overridden with "<inN.bin>.shape" (comma separated dims).
//   - Each output is dumped to "<model.onnx>.outN.bin" (float32) for golden comparison.
//
// Build: see scripts/build-ort-probe.ps1 (needs the OHOS NDK + src-tauri/libs/ohos/arm64-v8a).
// Deploy: copy the binary plus libonnxruntime.so, libonnxruntime.so.1 (copy of the same file,
//         the SONAME is .so.1) and libc++_shared.so into the same directory on device, e.g.
//         /data/local/tmp/ort/, then run with LD_LIBRARY_PATH=/data/local/tmp/ort.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "onnxruntime_c_api.h"

static const OrtApi *g = NULL;

static double now_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return ts.tv_sec * 1000.0 + ts.tv_nsec / 1e6;
}

static unsigned char *read_file(const char *path, size_t *size) {
  FILE *f = fopen(path, "rb");
  if (!f) return NULL;
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  unsigned char *buf = (unsigned char *)malloc(n > 0 ? n : 1);
  if (fread(buf, 1, n, f) != (size_t)n) { fclose(f); free(buf); return NULL; }
  fclose(f);
  *size = (size_t)n;
  return buf;
}

int main(int argc, char **argv) {
  if (argc < 5) {
    fprintf(stderr, "usage: %s <model.onnx> <iters> <threads> <in0.bin> [inN.bin...]\n", argv[0]);
    return 2;
  }
  g = OrtGetApiBase()->GetApi(ORT_API_VERSION);
  if (!g) { printf("FAIL: OrtApi\n"); return 1; }
  printf("[ort] api_version=%d\n", ORT_API_VERSION);
  fflush(stdout);

  OrtEnv *env = NULL;
  OrtSessionOptions *opts = NULL;
  OrtSession *sess = NULL;
  OrtStatus *st = NULL;
  int threads = atoi(argv[3]);
  int iters = atoi(argv[2]);

  if ((st = g->CreateEnv(ORT_LOGGING_LEVEL_ERROR, "ort_probe", &env))) goto fail;
  if ((st = g->CreateSessionOptions(&opts))) goto fail;
  if ((st = g->SetSessionGraphOptimizationLevel(opts, ORT_ENABLE_ALL))) goto fail;
  if ((st = g->SetIntraOpNumThreads(opts, threads))) goto fail;
  if ((st = g->SetInterOpNumThreads(opts, 1))) goto fail;

  double t0 = now_ms();
  if ((st = g->CreateSession(env, argv[1], opts, &sess))) goto fail;
  printf("[load] %.1f ms (threads=%d)\n", now_ms() - t0, threads);
  fflush(stdout);

  size_t nin = 0;
  if ((st = g->SessionGetInputCount(sess, &nin))) goto fail;

  OrtValue **vals = (OrtValue **)calloc(nin, sizeof(OrtValue *));
  const char **names = (const char **)calloc(nin, sizeof(char *));
  OrtMemoryInfo *mem = NULL;
  if ((st = g->CreateCpuMemoryInfo(OrtArenaAllocator, OrtMemTypeDefault, &mem))) goto fail;
  OrtAllocator *alloc = NULL;
  if ((st = g->GetAllocatorWithDefaultOptions(&alloc))) goto fail;

  for (size_t i = 0; i < nin; i++) {
    char *name = NULL;
    if ((st = g->SessionGetInputName(sess, i, alloc, &name))) goto fail;
    OrtTypeInfo *ti = NULL;
    if ((st = g->SessionGetInputTypeInfo(sess, i, &ti))) goto fail;
    const OrtTensorTypeAndShapeInfo *tsi = NULL;
    if ((st = g->CastTypeInfoToTensorInfo(ti, &tsi))) goto fail;
    ONNXTensorElementDataType et;
    if ((st = g->GetTensorElementType(tsi, &et))) goto fail;
    size_t rank = 0;
    if ((st = g->GetDimensionsCount(tsi, &rank))) goto fail;
    int64_t dims[8] = {0};
    if ((st = g->GetDimensions(tsi, dims, rank))) goto fail;

    char shp[512];
    snprintf(shp, sizeof(shp), "%s.shape", argv[4 + i]);
    FILE *sf = fopen(shp, "r");
    if (sf) {
      char line[256];
      if (fgets(line, sizeof(line), sf)) {
        size_t k = 0;
        char *tok = strtok(line, ",");
        while (tok && k < rank) { dims[k++] = atoll(tok); tok = strtok(NULL, ","); }
      }
      fclose(sf);
      printf("[shape] %s <- %s\n", name, shp);
    }
    size_t count = 1;
    for (size_t k = 0; k < rank; k++) {
      if (dims[k] <= 0) dims[k] = 1;
      count *= (size_t)dims[k];
    }
    size_t elem = et == ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT ? 4
                : et == ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT8 ? 1
                : et == ONNX_TENSOR_ELEMENT_DATA_TYPE_INT64 ? 8 : 4;
    size_t bytes = 0;
    unsigned char *data = read_file(argv[4 + i], &bytes);
    if (!data) { printf("FAIL: open %s\n", argv[4 + i]); return 1; }
    if (bytes < count * elem) { printf("FAIL: %s size %zu < %zu\n", argv[4 + i], bytes, count * elem); return 1; }
    if ((st = g->CreateTensorWithDataAsOrtValue(mem, data, count * elem, dims, rank, et, &vals[i]))) goto fail;
    names[i] = name;  /* kept alive for Run */
    printf("[in %zu] %s rank=%zu elems=%zu\n", i, name, rank, count);
    fflush(stdout);
    g->ReleaseTypeInfo(ti);
  }

  size_t nout = 0;
  if ((st = g->SessionGetOutputCount(sess, &nout))) goto fail;
  char **onames = (char **)calloc(nout, sizeof(char *));
  for (size_t i = 0; i < nout; i++) {
    if ((st = g->SessionGetOutputName(sess, i, alloc, &onames[i]))) goto fail;
  }
  OrtValue **ovals = (OrtValue **)calloc(nout, sizeof(OrtValue *));

  double tw = now_ms();
  if ((st = g->Run(sess, NULL, names, (const OrtValue *const *)vals, nin, (const char *const *)onames, nout, ovals))) goto fail;
  printf("[warmup] %.1f ms\n", now_ms() - tw);
  fflush(stdout);

  double best = 1e30, sum = 0;
  for (int it = 0; it < iters; it++) {
    double t = now_ms();
    if ((st = g->Run(sess, NULL, names, (const OrtValue *const *)vals, nin, (const char *const *)onames, nout, ovals))) goto fail;
    double dt = now_ms() - t;
    if (dt < best) best = dt;
    sum += dt;
    printf("[run %d/%d] %.1f ms\n", it + 1, iters, dt);
    fflush(stdout);
  }

  for (size_t i = 0; i < nout; i++) {
    OrtTensorTypeAndShapeInfo *oti = NULL;
    if (g->GetTensorTypeAndShape(ovals[i], &oti) != NULL) continue;
    size_t cnt = 0;
    g->GetTensorShapeElementCount(oti, &cnt);
    void *raw = NULL;
    if (g->GetTensorMutableData(ovals[i], &raw) != NULL) continue;
    char path[768];
    snprintf(path, sizeof(path), "%s.out%zu.bin", argv[1], i);
    FILE *f = fopen(path, "wb");
    if (f) { fwrite(raw, 1, cnt * 4, f); fclose(f); printf("[dump] %s elems=%zu\n", path, cnt); }
    g->ReleaseTensorTypeAndShapeInfo(oti);
  }
  printf("RESULT min=%.1f mean=%.1f ms\n", best, sum / iters);
  printf("PASS\n");
  return 0;
fail:
  printf("FAIL: %s\n", st ? g->GetErrorMessage(st) : "unknown");
  return 1;
}
