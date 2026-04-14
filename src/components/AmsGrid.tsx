import { AmsSlot } from "../stores/appStore";

interface AmsGridProps {
  ams: Record<string, AmsSlot>;
}

export default function AmsGrid({ ams }: AmsGridProps) {
  const slots = Object.entries(ams).slice(0, 4);
  const emptySlots = Array.from({ length: 4 }, (_, i) => i);

  return (
    <div className="ams-section">
      <div className="ams-title">AMS 料槽</div>
      <div className="ams-grid">
        {emptySlots.map((idx) => {
          const slot = slots[idx];
          const slotData = slot?.[1];

          return (
            <div className="ams-slot" key={idx}>
              <div className="ams-slot-header">槽位 {idx + 1}</div>
              {slotData ? (
                <>
                  <div className="ams-color">
                    <div
                      className="ams-dot"
                      style={{ background: slotData.color || "#888" }}
                    ></div>
                    <div className="ams-mat">{slotData.material || "—"}</div>
                  </div>
                  <div className="ams-remain">
                    {slotData.remaining != null ? `${slotData.remaining}%` : "—"}
                  </div>
                </>
              ) : (
                <div className="ams-mat">—</div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
