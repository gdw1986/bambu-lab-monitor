import { PrinterState } from "../stores/appStore";

interface TempGaugesProps {
  state: PrinterState;
}

const CIRC = 2 * Math.PI * 45;

function tempClass(actual: number, target: number): { cls: string; arcCls: string } {
  if (target === 0) return { cls: "c", arcCls: "cool" };
  const ratio = actual / target;
  if (ratio >= 0.98) return { cls: "h", arcCls: "hot" };
  if (ratio >= 0.7) return { cls: "w", arcCls: "warm" };
  return { cls: "c", arcCls: "cool" };
}

function arcPct(val: number, target: number, maxT = 300): number {
  const ratio = target > 0 ? Math.min(val / target, 1.5) : Math.min(val / maxT, 1);
  return ratio * CIRC;
}

export default function TempGauges({ state }: TempGaugesProps) {
  const { nozzle_temp, nozzle_target, bed_temp, bed_target, live_speed } = state;

  const nc = tempClass(nozzle_temp, nozzle_target);
  const bc = tempClass(bed_temp, bed_target);
  const spdArc = live_speed > 200 ? "hot" : live_speed > 100 ? "warm" : "cool";

  return (
    <div className="temps-row">
      <div className="gauge-card slide-up" style={{ animationDelay: ".06s" }}>
        <div className="gauge-label">喷嘴</div>
        <div className="gauge-wrap">
          <svg className="gauge-svg" viewBox="0 0 100 100">
            <circle className="gauge-bg" cx="50" cy="50" r="45" />
            <circle
              className={`gauge-fg ${nc.arcCls}`}
              cx="50"
              cy="50"
              r="45"
              style={{
                strokeDasharray: `${arcPct(nozzle_temp, nozzle_target)} ${CIRC}`,
              }}
            />
          </svg>
          <div className="gauge-center">
            <span className={`gauge-temp ${nc.cls}`}>
              {nozzle_temp > 0 ? `${nozzle_temp}°` : "—"}
            </span>
            <span className="gauge-tgt">
              {nozzle_target > 0 ? `${nozzle_target}°C →` : "—"}
            </span>
          </div>
        </div>
      </div>

      <div className="gauge-card slide-up" style={{ animationDelay: ".1s" }}>
        <div className="gauge-label">热床</div>
        <div className="gauge-wrap">
          <svg className="gauge-svg" viewBox="0 0 100 100">
            <circle className="gauge-bg" cx="50" cy="50" r="45" />
            <circle
              className={`gauge-fg ${bc.arcCls}`}
              cx="50"
              cy="50"
              r="45"
              style={{
                strokeDasharray: `${arcPct(bed_temp, bed_target, 120)} ${CIRC}`,
              }}
            />
          </svg>
          <div className="gauge-center">
            <span className={`gauge-temp ${bc.cls}`}>
              {bed_temp > 0 ? `${bed_temp}°` : "—"}
            </span>
            <span className="gauge-tgt">
              {bed_target > 0 ? `${bed_target}°C →` : "—"}
            </span>
          </div>
        </div>
      </div>

      <div className="gauge-card slide-up" style={{ animationDelay: ".14s" }}>
        <div className="gauge-label">速度档</div>
        <div className="gauge-wrap">
          <svg className="gauge-svg" viewBox="0 0 100 100">
            <circle className="gauge-bg" cx="50" cy="50" r="45" />
            <circle
              className={`gauge-fg ${spdArc}`}
              cx="50"
              cy="50"
              r="45"
              style={{
                strokeDasharray: `${Math.min(live_speed / 300, 1) * CIRC} ${CIRC}`,
              }}
            />
          </svg>
          <div className="gauge-center">
            <span className={`gauge-temp ${spdArc}`}>
              {live_speed > 0 ? live_speed : "—"}
            </span>
            <span className="gauge-tgt">mm/s</span>
          </div>
        </div>
      </div>
    </div>
  );
}
