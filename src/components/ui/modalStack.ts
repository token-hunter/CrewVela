/**
 * Module-level modal stack: the last mounted dialog is the only one allowed
 * to consume Escape. App.tsx's legacy Esc chain yields entirely while any
 * Modal is open (hasOpenModal), preserving top-down close priority.
 */
export interface ModalStackEntry {
  id: number;
}

const stack: ModalStackEntry[] = [];
let nextModalId = 1;

/** Push a new modal; returns its id. The newest id is the stack top. */
export function pushModal(): number {
  const id = nextModalId++;
  stack.push({ id });
  return id;
}

/** Remove a modal by id (no-op if absent, e.g. already unmounted). */
export function removeModal(id: number): void {
  const index = stack.findIndex((entry) => entry.id === id);
  if (index >= 0) stack.splice(index, 1);
}

/** Only the stack-top modal may respond to Escape. */
export function isTopModal(id: number): boolean {
  return stack.length > 0 && stack[stack.length - 1].id === id;
}

/** True while any Modal-managed dialog is mounted. */
export function hasOpenModal(): boolean {
  return stack.length > 0;
}

/** Number of currently mounted modals. */
export function modalStackSize(): number {
  return stack.length;
}

/** Stack ids bottom-to-top (test hook). */
export function modalStackSnapshot(): number[] {
  return stack.map((entry) => entry.id);
}
