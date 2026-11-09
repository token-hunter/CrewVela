import { useMemo, useRef, useState } from "react";
import { CaretDownIcon } from "@phosphor-icons/react/dist/csr/CaretDown";

export interface ModelComboboxOption {
  value: string;
  hint?: string | null;
}

interface Props {
  id?: string;
  value: string;
  options: ModelComboboxOption[];
  placeholder?: string;
  onChange: (value: string) => void;
}

export function ModelCombobox({ id, value, options, placeholder, onChange }: Props) {
  const rootRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);

  const visibleOptions = useMemo(() => {
    const query = value.trim().toLowerCase();
    if (!query || options.some((option) => option.value.toLowerCase() === query)) {
      return options;
    }
    return options.filter((option) => option.value.toLowerCase().includes(query));
  }, [options, value]);

  const select = (option: ModelComboboxOption) => {
    onChange(option.value);
    setOpen(false);
  };

  const scrollHighlightIntoView = (index: number) => {
    listRef.current
      ?.querySelectorAll(".model-combobox-option")
      [index]?.scrollIntoView({ block: "nearest" });
  };

  const moveHighlight = (delta: number) => {
    if (visibleOptions.length === 0) return;
    setHighlight((current) => {
      const next = Math.min(Math.max(current + delta, 0), visibleOptions.length - 1);
      scrollHighlightIntoView(next);
      return next;
    });
  };

  return (
    <div
      ref={rootRef}
      className={`model-combobox${open ? " open" : ""}`}
      onBlur={(event) => {
        if (!rootRef.current?.contains(event.relatedTarget as Node | null)) {
          setOpen(false);
        }
      }}
    >
      <input
        id={id}
        role="combobox"
        aria-expanded={open}
        aria-autocomplete="list"
        value={value}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
        onFocus={() => {
          setOpen(true);
          setHighlight(0);
        }}
        onChange={(event) => {
          onChange(event.target.value);
          setOpen(true);
          setHighlight(0);
        }}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing || event.keyCode === 229) return;
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setOpen(true);
            moveHighlight(1);
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            moveHighlight(-1);
          } else if (event.key === "Enter") {
            if (open && visibleOptions[highlight]) {
              event.preventDefault();
              select(visibleOptions[highlight]);
            }
          } else if (event.key === "Escape") {
            if (open) { event.preventDefault(); event.stopPropagation(); }
            setOpen(false);
          }
        }}
      />
      <button
        type="button"
        tabIndex={-1}
        aria-label="展开模型列表"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          setOpen((current) => !current);
          setHighlight(0);
        }}
      >
        <CaretDownIcon size={14} aria-hidden="true" />
      </button>
      {open && (
        <div className="model-combobox-list" role="listbox" ref={listRef} onMouseDown={(event) => event.preventDefault()}>
          {visibleOptions.length === 0 ? (
            <div className="model-combobox-empty">
              {options.length === 0 ? "点击右上角“获取模型”拉取列表" : "没有匹配的模型"}
            </div>
          ) : (
            visibleOptions.map((option, index) => (
              <div
                key={option.value}
                role="option"
                aria-selected={option.value === value}
                className={`model-combobox-option${index === highlight ? " active" : ""}`}
                onMouseEnter={() => setHighlight(index)}
                onClick={() => select(option)}
              >
                <span>{option.value}</span>
                {option.hint && <em>{option.hint}</em>}
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}
