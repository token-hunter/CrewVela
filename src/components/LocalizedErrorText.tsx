import { useId, useState } from "react";
import { translate } from "../interfaceLanguage";
import { localizeError } from "../localizedError";
import { useInterfaceLanguage } from "../useInterfaceLanguage";

export function LocalizedErrorText({ source, fallback }: { source: string; fallback?: string }) {
  const language = useInterfaceLanguage();
  const { message, details } = localizeError(language, source, fallback);
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();

  if (!details) return <>{message}</>;
  return <>
    {message}{" "}
    <button type="button" className="localized-error-toggle" aria-expanded={expanded}
      aria-controls={detailsId} onClick={() => setExpanded(open => !open)}>
      {translate(language, "错误详情")}
    </button>
    <span id={detailsId} className="localized-error-details" hidden={!expanded}>{details}</span>
  </>;
}
