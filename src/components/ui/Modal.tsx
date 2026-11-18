import {
  useEffect,
  useRef,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";
import {
  hasOpenModal,
  isTopModal,
  pushModal,
  removeModal,
} from "./modalStack";

export { hasOpenModal } from "./modalStack";

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

export interface ModalProps {
  onClose: () => void;
  /** Dialog container class, e.g. "settings-dialog". */
  className: string;
  /** Overlay class; defaults to "dialog-overlay". */
  overlayClassName?: string;
  /** aria-labelledby target id. */
  labelledBy?: string;
  /** aria-label for the dialog. */
  ariaLabel?: string;
  /** Overlay click closes the dialog (per-dialog opt-in; default false). */
  closeOnOverlayClick?: boolean;
  /** Explicit initial focus; otherwise the first focusable element. */
  initialFocusRef?: RefObject<HTMLElement | null>;
  style?: CSSProperties;
  children: ReactNode;
}

export function Modal({
  onClose,
  className,
  overlayClassName = "dialog-overlay",
  labelledBy,
  ariaLabel,
  closeOnOverlayClick = false,
  initialFocusRef,
  style,
  children,
}: ModalProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    const id = pushModal();
    const previouslyFocused = document.activeElement as HTMLElement | null;

    // Initial focus: explicit ref wins, else first focusable, else the container.
    const container = dialogRef.current;
    const target =
      initialFocusRef?.current ??
      container?.querySelector<HTMLElement>(FOCUSABLE_SELECTOR) ??
      container;
    target?.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (!isTopModal(id)) return;
      if (event.isComposing || event.keyCode === 229) return;
      if (event.key === "Escape") {
        if (event.target instanceof Element && event.target.closest('.model-combobox.open')) return;
        // Close an open local disclosure before its containing dialog, including
        // keyboard events dispatched from the dialog rather than its summary.
        const disclosureSelector = '.skill-import-menu[open], .skill-tools-menu[open], .skill-filter-menu[open], .automation-card-menu[open], .model-provider-more[open], .collab-member-actions[open]';
        const focusedDisclosure = event.target instanceof Element ? event.target.closest<HTMLDetailsElement>(disclosureSelector) : null;
        const importMenu = focusedDisclosure && dialogRef.current?.contains(focusedDisclosure)
          ? focusedDisclosure : dialogRef.current?.querySelector<HTMLDetailsElement>(disclosureSelector);
        if (importMenu) {
          event.preventDefault();
          event.stopPropagation();
          importMenu.open = false;
          importMenu.querySelector('summary')?.focus();
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const root = dialogRef.current;
      if (!root) return;
      const focusable = Array.from(
        root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
      ).filter(
        (el) => !el.matches(":disabled") && (el.getClientRects().length > 0 || el === document.activeElement),
      );
      if (focusable.length === 0) {
        event.preventDefault();
        root.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (event.shiftKey) {
        if (active === first || !root.contains(active)) {
          event.preventDefault();
          last.focus();
        }
      } else if (active === last || !root.contains(active)) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown, { capture: true });

    return () => {
      window.removeEventListener("keydown", onKeyDown, { capture: true });
      removeModal(id);
      // Return focus to wherever the dialog stole it from.
      if (
        previouslyFocused &&
        document.contains(previouslyFocused) &&
        !dialogRef.current?.contains(document.activeElement)
      ) {
        previouslyFocused.focus();
      }
    };
  }, [initialFocusRef]);

  return (
    <div
      className={overlayClassName}
      onClick={closeOnOverlayClick ? () => onCloseRef.current() : undefined}
    >
      <div
        ref={dialogRef}
        className={className}
        role="dialog"
        aria-modal="true"
        aria-label={ariaLabel}
        aria-labelledby={labelledBy}
        tabIndex={-1}
        style={style}
        onClick={(event) => event.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}
