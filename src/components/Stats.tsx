import { PrinterState } from "../stores/appStore";

interface StatsProps {
  state: PrinterState;
}

function fmtTime(secs: number): string {
  if (!secs || secs <= 0) return "—";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (h > 0) return `${h}小时${m}分`;
  return `${m}分${s}秒`;
}

export default function Stats({ state }: StatsProps) {
  const { gcode_state, remaining_time, layer_current, layer_total, filament_type } = state;

  const stateLower = gcode_state.toLowerCase();
  const badges: Record<string, string> = {
    printing: "badge-print",
    running: "badge-print",
    idle: "badge-idle",
    pause: "badge-pause",
    paused: "badge-pause",
    finish: "badge-finish",
    completed: "badge-finish",
    success: "badge-finish",
    error: "badge-error",
    fail: "badge-error",
    cooling: "badge-cool",
  };
  const labels: Record<string, string> = {
    printing: "🖨 打印中",
    running: "🖨 打印中",
    idle: "⏸ 待机",
    pause: "⏸ 已暂停",
    paused: "⏸ 已暂停",
    finish: "✅ 已完成",
    completed: "✅ 已完成",
    success: "✅ 已完成",
    error: "❌ 错误",
    fail: "❌ 失败",
    cooling: "❄ 降温中",
  };

  const badgeClass = badges[stateLower] || "badge-default";
  const badgeLabel = labels[stateLower] || gcode_state;

  return (
    <div className="stats-row">
      <div className="stat-card slide-up" style={{ animationDelay: ".16s" }}>
        <div className="stat-label">状态</div>
        <div className="state-row">
          <span className={`badge ${badgeClass}`}>{badgeLabel}</span>
        </div>
      </div>
      <div className="stat-card slide-up" style={{ animationDelay: ".2s" }}>
        <div className="stat-label">剩余时间</div>
        <div className="stat-val m">{fmtTime(remaining_time)}</div>
        <div className="stat-sub">时:分:秒</div>
      </div>
      <div className="stat-card slide-up" style={{ animationDelay: ".24s" }}>
        <div className="stat-label">层</div>
        <div className="stat-val m">
          {layer_current && layer_total ? `${layer_current} / ${layer_total}` : "— / —"}
        </div>
        <div className="stat-sub">当前 / 总计</div>
      </div>
      <div className="stat-card slide-up" style={{ animationDelay: ".28s" }}>
        <div className="stat-label">当前耗材</div>
        <div className="stat-val m">{filament_type || "—"}</div>
        <div className="stat-sub">类型</div>
      </div>
    </div>
  );
}
