import clsx from 'clsx';
import type { MouseEventHandler, PointerEventHandler } from 'react';
import { Orientation } from './AppProperties';

interface ResizerProps {
  direction: Orientation;
  onMouseDown: PointerEventHandler<HTMLDivElement>;
  onDoubleClick?: MouseEventHandler<HTMLDivElement>;
  /** Show a grab pill and grow the strip: the 8px hit box is not finger-sized.
   *  Only used by the compact (phone) editor sheet. */
  showHandle?: boolean;
}

const Resizer = ({ direction, onMouseDown, onDoubleClick, showHandle = false }: ResizerProps) => (
  <div
    className={clsx('shrink-0 bg-transparent z-10 touch-none flex items-center justify-center', {
      'cursor-col-resize': direction === Orientation.Vertical,
      'cursor-row-resize': direction === Orientation.Horizontal,
      'w-2': direction === Orientation.Vertical && !showHandle,
      'w-3.5': direction === Orientation.Vertical && showHandle,
      'h-2': direction === Orientation.Horizontal && !showHandle,
      'h-3.5': direction === Orientation.Horizontal && showHandle,
    })}
    role="separator"
    aria-orientation={direction === Orientation.Vertical ? 'vertical' : 'horizontal'}
    onPointerDown={onMouseDown}
    onDoubleClick={onDoubleClick}
    style={{ touchAction: 'none' }}
  >
    {showHandle && (
      <span
        className={clsx('rounded-full bg-border-color', direction === Orientation.Horizontal ? 'h-1 w-10' : 'w-1 h-10')}
        aria-hidden
      />
    )}
  </div>
);

export default Resizer;
