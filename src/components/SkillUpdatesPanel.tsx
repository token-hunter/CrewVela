import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "../skillUpdates.css";

interface Asset {
  id: string;
  name: string;
  sourceType: string;
  sourceRevision: string | null;
  contentHash: string;
  sourceRef: string | null;
  riskSummary: string[];
}
interface Deployment {
  id: string;
  skillId: string;
  runtime: string;
  scope: string;
  targetPath: string;
  sourceHash: string;
  status: string;
}
interface Check {
  id: string;
  skillId: string;
  status: string;
  message: string;
  checkedAt: number;
  baseHash: string;
  candidate: Asset | null;
  omittedFiles: number;
  changes: Array<{
    path: string;
    kind: string;
    before: string | null;
    after: string | null;
    truncated: boolean;
  }>;
}
interface Version {
  id: string;
  savedAt: number;
  asset: Asset;
}
interface Props {
  assets: Asset[];
  deployments: Deployment[];
  initialSkillId: string | null;
  onChanged: () => Promise<void>;
}
const labels: Record<string, string> = {
  available: "可更新 / 待确认",
  up_to_date: "已是最新内容",
  pinned: "固定 Commit",
  manual: "需手动提供新版",
  error: "检查失败",
  applied: "中央库已更新",
};
const stamp = (value: number) => new Date(value * 1000).toLocaleString();

export function SkillUpdatesPanel({
  assets,
  deployments,
  initialSkillId,
  onChanged,
}: Props) {
  const [checks, setChecks] = useState<Check[]>([]);
  const [selected, setSelected] = useState(
    initialSkillId ?? assets[0]?.id ?? "",
  );
  const [versions, setVersions] = useState<Version[]>([]);
  const [busy, setBusy] = useState(false);
  const operationInFlight = useRef(false);
  const reloadSequence = useRef(0);
  const [error, setError] = useState("");
  const [info, setInfo] = useState("");
  const [reviewed, setReviewed] = useState(false);
  const [targets, setTargets] = useState<string[]>([]);
  const asset = assets.find((item) => item.id === selected);
  const check = checks.find((item) => item.skillId === selected);
  const deployed = deployments.filter((item) => item.skillId === selected);

  const reload = async () => {
    const sequence = ++reloadSequence.current;
    const next = await invoke<Check[]>("skill_center_update_checks");
    if (sequence === reloadSequence.current) setChecks(next);
  };
  useEffect(() => {
    void reload().catch((e) => setError(String(e)));
  }, []);
  useEffect(() => {
    let alive = true;
    setVersions([]);
    setTargets([]);
    setReviewed(false);
    if (selected)
      void invoke<Version[]>("skill_center_versions", { skillId: selected })
        .then((items) => {
          if (alive) setVersions(items);
        })
        .catch((e) => {
          if (alive) setError(String(e));
        });
    return () => {
      alive = false;
    };
  }, [selected, asset?.contentHash]);

  const run = async (work: () => Promise<void>) => {
    if (operationInFlight.current) return;
    operationInFlight.current = true;
    setBusy(true);
    setError("");
    setInfo("");
    try {
      await work();
      await reload();
    } catch (e) {
      setError(String(e));
    } finally {
      operationInFlight.current = false;
      setBusy(false);
    }
  };
  const checkSource = (
    id: string | null,
    replacementArchive: string | null = null,
  ) =>
    run(async () => {
      setReviewed(false);
      await invoke("skill_center_check_updates", {
        skillId: id,
        replacementArchive,
      });
    });
  const archive = async () => {
    if (!asset) return;
    await run(async () => {
      const path = await open({ multiple: false, filters: [{ name: "Skill 压缩包", extensions: ["zip", "skill"] }] });
      if (typeof path === "string") { setReviewed(false); await invoke("skill_center_check_updates", { skillId: asset.id, replacementArchive: path }); }
    });
  };
  const apply = () =>
    run(async () => {
      if (!asset || !check || !reviewed) return;
      await invoke("skill_center_apply_update", {
        skillId: asset.id,
        checkId: check.id,
        trustConfirmed: reviewed,
      });
      await onChanged();
      setTargets([]);
      setReviewed(false);
      setVersions(
        await invoke<Version[]>("skill_center_versions", { skillId: asset.id }),
      );
      setInfo("中央库已切换版本；未选择的 Runtime 继续使用旧版本。");
    });
  const sync = () =>
    run(async () => {
      if (!asset || !targets.length) return;
      if (
        !window.confirm(
          `同步 ${targets.length} 个部署到当前中央版本？本地修改或未选中的共享目录会阻止整批操作。`,
        )
      )
        return;
      await invoke("skill_center_sync_update", {
        skillId: asset.id,
        deploymentIds: targets,
        expectedHash: asset.contentHash,
      });
      await onChanged();
      setTargets([]);
      setInfo("所选 Runtime 部署已同步。");
    });

  return (
    <section className="skill-updates" aria-label="Skill 版本更新">
      <header>
        <div>
          <h3>版本检测与更新</h3>
          <p>
            先审阅中央库候选版本，再选择 Runtime 同步。固定 Commit 不自动追新。
          </p>
        </div>
        <button
          className="btn primary"
          disabled={busy || !assets.length}
          onClick={() => void checkSource(null)}
        >
          {busy ? "处理中…" : "检查全部更新"}
        </button>
      </header>
      {error && (
        <div role="alert" className="skill-center-message error">
          {error}
        </div>
      )}
      {info && (
        <div role="status" className="skill-center-message success">
          {info}
        </div>
      )}
      <div className="skill-update-layout">
        <aside aria-label="Skill 更新列表">
          {assets.map((item) => {
            const status = checks.find((c) => c.skillId === item.id);
            return (
              <article
                key={item.id}
                className={selected === item.id ? "selected" : ""}
              >
                <button disabled={busy} onClick={() => { if (!operationInFlight.current) setSelected(item.id); }}>
                  <strong>{item.name}</strong>
                  <small>
                    {status
                      ? (labels[status.status] ?? status.status)
                      : "尚未检查"}
                  </small>
                </button>
                <button
                  className="btn"
                  disabled={busy}
                  onClick={() => {
                    if (operationInFlight.current) return;
                    setSelected(item.id);
                    void checkSource(item.id);
                  }}
                >
                  检查更新
                </button>
              </article>
            );
          })}
          {!assets.length && <p>请先导入 Skill。</p>}
        </aside>
        <main>
          {asset ? (
            <>
              <h3>{asset.name}</h3>
              <p className="skill-update-source">
                来源：{asset.sourceRef ?? "未知"}
              </p>
              <p>
                当前：{asset.sourceRevision?.slice(0, 12) ?? "本地版本"} ·{" "}
                {asset.contentHash.slice(0, 12)}
              </p>
              {asset.sourceType === "archive" && (
                <button
                  className="btn"
                  disabled={busy}
                  onClick={() => void archive()}
                >
                  选择新版压缩包并比较
                </button>
              )}
              {check && (
                <section className="skill-update-review">
                  <h4>{labels[check.status] ?? check.status}</h4>
                  <p>{check.message}</p>
                  <small>检查时间：{stamp(check.checkedAt)}</small>
                  {check.candidate && (
                    <p>
                      候选：
                      {check.candidate.sourceRevision?.slice(0, 12) ??
                        "内容版本"}{" "}
                      · {check.candidate.contentHash.slice(0, 12)}
                    </p>
                  )}
                  {check.candidate?.riskSummary.map((risk) => (
                    <p className="skill-update-risk" key={risk}>
                      {risk}
                    </p>
                  ))}
                  {check.omittedFiles > 0 && (
                    <p className="skill-update-risk">
                      另有 {check.omittedFiles}{" "}
                      个变更文件未展开。请完整审阅来源再确认。
                    </p>
                  )}
                  {check.changes.map((change) => (
                    <details
                      key={change.path}
                      open={change.path.toLowerCase() === "skill.md"}
                    >
                      <summary>
                        {change.kind === "added"
                          ? "新增"
                          : change.kind === "deleted"
                            ? "删除"
                            : "修改"}{" "}
                        · {change.path}
                        {change.truncated ? "（非完整文本）" : ""}
                      </summary>
                      <div className="skill-update-diff">
                        <div>
                          <strong>当前版本</strong>
                          <pre>{change.before ?? "该文件不存在"}</pre>
                        </div>
                        <div>
                          <strong>候选版本</strong>
                          <pre>{change.after ?? "该文件已删除"}</pre>
                        </div>
                      </div>
                    </details>
                  ))}
                  {check.status === "available" && (
                    <footer>
                      <label>
                        <input
                          type="checkbox"
                          checked={reviewed}
                          disabled={busy}
                          onChange={(e) => setReviewed(e.target.checked)}
                        />
                        我已审阅文件差异与脚本风险，信任此版本
                      </label>
                      <button
                        className="btn primary"
                        disabled={busy || !reviewed}
                        onClick={() => void apply()}
                      >
                        确认切换中央版本
                      </button>
                    </footer>
                  )}
                </section>
              )}
              <section className="skill-update-targets">
                <h4>选择 Runtime 同步</h4>
                <p>
                  更新仅影响勾选的已有部署；需要新增部署请使用分发矩阵。同步后使用独立副本，避免其它
                  Runtime 被连带升级。
                </p>
                {deployed.map((item) => (
                  <label key={item.id}>
                    <input
                      type="checkbox"
                      checked={targets.includes(item.id)}
                      disabled={busy || item.runtime === "oh_my_pi"}
                      onChange={(e) =>
                        setTargets((old) =>
                          e.target.checked
                            ? [...old, item.id]
                            : old.filter((id) => id !== item.id),
                        )
                      }
                    />
                    <span>
                      <strong>
                        {item.runtime === "oh_my_pi" ? "Oh My Pi（已移除）" : item.runtime} · {item.scope}
                      </strong>
                      <small>{item.targetPath}</small>
                      <small>
                        {item.sourceHash === asset.contentHash
                          ? "当前中央版本"
                          : "旧版本"}{" "}
                        · {item.status}
                      </small>
                    </span>
                  </label>
                ))}
                {!deployed.length && <p>尚无 Runtime 部署。</p>}
                <button
                  className="btn"
                  disabled={busy || !targets.length}
                  onClick={() => void sync()}
                >
                  同步所选部署（{targets.length}）
                </button>
              </section>
              <section className="skill-update-versions">
                <h4>版本历史 / 回滚</h4>
                {versions.map((version) => (
                  <article key={version.id}>
                    <span>
                      {stamp(version.savedAt)} ·{" "}
                      {version.asset.contentHash.slice(0, 12)}
                    </span>
                    <button
                      className="btn"
                      disabled={
                        busy || version.asset.contentHash === asset.contentHash
                      }
                      onClick={() =>
                        void run(async () => {
                          setReviewed(false);
                          await invoke("skill_center_prepare_rollback", {
                            skillId: asset.id,
                            versionId: version.id,
                          });
                        })
                      }
                    >
                      预览回滚差异
                    </button>
                  </article>
                ))}
                {!versions.length && <p>首次更新后会保留旧版本。</p>}
              </section>
            </>
          ) : (
            <p>选择一个 Skill 检查来源版本。</p>
          )}
        </main>
      </div>
    </section>
  );
}
