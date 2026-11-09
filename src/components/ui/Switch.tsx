import type { ReactNode } from "react";

export interface SwitchProps {
  className: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  /** aria-label for the wrapper (variants whose row text lives outside the switch). */
  label?: string;
  /** Hover tooltip, e.g. the mcp-toggle enabled/disabled hint. */
  title?: string;
  /** "span" for switches nested inside an outer row label. */
  wrapper?: "label" | "span";
  /** Extra variant content (text spans, strong name, …). */
  children?: ReactNode;
  /** Whether children render before or after the input. */
  childrenPosition?: "before" | "after";
  /** Render the decorative knob <i>. */
  withKnob?: boolean;
}

export function Switch({
  className,
  checked,
  onChange,
  disabled,
  label,
  title,
  wrapper = "label",
  children,
  childrenPosition = "after",
  withKnob = true,
}: SwitchProps) {
  const Wrapper = wrapper;
  return (
    <Wrapper className={className} aria-label={label} title={title}>
      {childrenPosition === "before" ? children : null}
      <input
        type="checkbox"
        role="switch"
        aria-checked={checked}
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      {withKnob ? <i aria-hidden="true" /> : null}
      {childrenPosition === "after" ? children : null}
    </Wrapper>
  );
}
