import { PrinterState } from "../stores/appStore";

interface ProgressHeroProps {
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

export default function ProgressHero({ state }: ProgressHeroProps) {
  const { gcode_state, progress, job_name, remaining_time, layer_current, layer_total, speed, last_update } = state;
  
  const isPrinting = ["RUNNING", "打印中"].includes(gcode_state);
  const isFinish = ["FINISH", "COMPLETED", "SUCCESS"].includes(gcode_state);
  
  let badgeClass = "hero-state-badge idle";
  let stateText = "待机";
  if (isFinish) {
    badgeClass = "hero-state-badge finish";
    stateText = "已完成";
  } else if (isPrinting) {
    badgeClass = "hero-state-badge";
    stateText = "打印中";
  }

  return (
    <div className="hero-progress slide-up">
      <div className="hero-top">
        <div className="hero-left">
          <div className={badgeClass}>
            <span className="dot-pulse"></span>
            <span>{stateText}</span>
          </div>
          <div className={`hero-job-name ${!job_name ? "empty" : ""}`}>
            {job_name || "暂无打印任务"}
          </div>
        </div>
        <div className="hero-right">
          <div className="hero-pct">{Math.round(progress)}%</div>
          <div className="hero-pct-label">完成</div>
        </div>
      </div>
      <div className="hero-bar-wrap">
        <div className="hero-bar">
          <div className="hero-bar-fill" style={{ width: `${progress}%` }}></div>
        </div>
      </div>
      <div className="hero-meta">
        <div className="hero-meta-item">
          <span className="hero-meta-label">预计完成</span>
          <span className="hero-meta-val hero-eta-val">{fmtTime(remaining_time)}</span>
        </div>
        <div className="hero-meta-item">
          <span className="hero-meta-label">层</span>
          <span className="hero-meta-val hero-layer-val">
            {layer_current && layer_total ? `${layer_current} / ${layer_total}` : "—"}
          </span>
        </div>
        <div className="hero-meta-item">
          <span className="hero-meta-label">速度档</span>
          <span className="hero-meta-val">{speed || "—"}</span>
        </div>
        <div className="hero-meta-item">
          <span className="hero-meta-label">剩余时间</span>
          <span className="hero-meta-val hero-eta-val">{fmtTime(remaining_time)}</span>
        </div>
        <div className="hero-meta-item">
          <span className="hero-meta-label">更新时间</span>
          <span className="hero-meta-val">{last_update || "—"}</span>
        </div>
      </div>
    </div>
  );
}
