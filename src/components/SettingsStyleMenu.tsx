import { useTranslation } from "../useInterfaceLanguage";
import { useEffect, useRef, useState } from "react";
import { Check, Palette } from "@phosphor-icons/react";
import "../styles/settings-creative-modes.css";

export interface SettingsStyleOption<T extends string> {
  id: T;
  label: string;
}

interface Props<T extends string> {
  label: string;
  value: T;
  options: ReadonlyArray<SettingsStyleOption<T>>;
  onChange: (next: T) => void;
  onOpenAppearanceSettings?: () => void;
}

export function SettingsStyleMenu<T extends string>({
  label,
  value,
  options,
  onChange,
  onOpenAppearanceSettings,
}: Props<T>) {
  const t = useTranslation();
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        setOpen(false);
      }
    };
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [open]);

  const current = options.find((item) => item.id === value) ?? options[0];

  return (
    <div className="cv-style-menu" ref={wrapRef}>
      <button
        type="button"
        className={`cv-style-trigger${open ? " open" : ""}${value !== "classic" ? " is-creative" : ""}`}
        aria-label={t("切换{layout}（当前：{current}）", { layout: t(label), current: t(current?.label ?? "") })}
        aria-haspopup="menu"
        aria-expanded={open}
        title={t("{layout}：{current}", { layout: t(label), current: t(current?.label ?? "") })}
        onClick={() => setOpen((prev) => !prev)}
      >
        <Palette size={13} aria-hidden="true" />
        <span>{t("模式")}</span>
      </button>

      {open && (
        <div className="cv-style-popover" role="menu" aria-label={t(label)}>
          <div className="cv-style-popover-title">{t(label)}</div>
          {options.map((option) => {
            const selected = option.id === value;
            return (
              <button
                key={option.id}
                type="button"
                role="menuitemradio"
                aria-checked={selected}
                className={`cv-style-option${selected ? " selected" : ""}`}
                onClick={() => {
                  onChange(option.id);
                  setOpen(false);
                }}
              >
                <span>{t(option.label)}</span>
                {selected && <Check size={13} weight="bold" aria-hidden="true" />}
              </button>
            );
          })}
          {onOpenAppearanceSettings && (
            <button
              type="button"
              className="cv-style-more"
              onClick={() => {
                setOpen(false);
                onOpenAppearanceSettings();
              }}
            >
              {t("前往外观设置")}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
