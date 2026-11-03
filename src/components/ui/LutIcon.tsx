import type { SVGProps } from 'react';

/**
 * LUT icon: an isometric colour cube.
 *
 * A 3D LUT *is* a colour cube — RGB input axes mapped to RGB output, so the
 * cube is the standard metaphor in grading tools. Three graded faces keep it
 * readable down to 14px, the accent-coloured edge hints "colour" without
 * fighting the theme, and the whole shape inherits `currentColor` so it works
 * in light and dark themes.
 */
export default function LutIcon({
  size = 16,
  colorful = true,
  className,
  ...props
}: SVGProps<SVGSVGElement> & { size?: number; colorful?: boolean }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
      {...props}
    >
      {/* top face */}
      <path d="M12 2.5 21 7.5 12 12.5 3 7.5Z" fill="currentColor" fillOpacity={0.18} />
      {/* left face */}
      <path d="M3 7.5 12 12.5 12 21.5 3 16.5Z" fill="currentColor" fillOpacity={0.1} />
      {/* right face */}
      <path d="M21 7.5 12 12.5 12 21.5 21 16.5Z" fill="currentColor" fillOpacity={0.05} />
      {/* colour hint along the front-right edge */}
      {colorful && (
        <path
          d="M12 12.5 21 7.5"
          stroke="currentColor"
          strokeOpacity={0.9}
          strokeWidth="1.75"
          style={{ color: 'var(--color-accent)' }}
        />
      )}
      {/* wireframe */}
      <path d="M12 2.5 21 7.5v9l-9 5-9-5v-9l9-5Z" />
      <path d="M3 7.5 12 12.5l9-5" />
      <path d="M12 12.5v9" />
    </svg>
  );
}
