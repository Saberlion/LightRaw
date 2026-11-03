import { useEffect, useRef } from 'react';

export function useCollapsibleHeight(isOpen: boolean) {
  const contentRef = useRef<HTMLDivElement | null>(null);
  const wrapperRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const wrapper = wrapperRef.current;
    const content = contentRef.current;
    if (!wrapper || !content) {
      return;
    }

    const updateMaxHeight = () => {
      if (isOpen) {
        const contentHeight = content.scrollHeight;
        wrapper.style.maxHeight = `${contentHeight}px`;
      } else {
        wrapper.style.maxHeight = '0px';
      }
    };

    updateMaxHeight();

    // ArkWeb hit-testing ignores the overflow clip: collapsed content still
    // swallows pointer events and stays in the accessibility tree with its
    // full laid-out bounds, creating invisible "dead zones" that eat taps meant
    // for whatever is rendered on top. `inert` removes the subtree from
    // hit-testing, focus and the a11y tree in one go.
    wrapper.inert = !isOpen;

    const resizeObserver = new ResizeObserver(updateMaxHeight);
    resizeObserver.observe(content);

    return () => resizeObserver.disconnect();
  }, [isOpen]);

  return { contentRef, wrapperRef };
}
