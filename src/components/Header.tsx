interface HeaderProps {
  onSettingsClick: () => void;
}

export default function Header({ onSettingsClick }: HeaderProps) {
  return (
    <header>
      <div className="logo">
        Bambu <span>//</span> Monitor
      </div>
      <button
        onClick={onSettingsClick}
        style={{
          background: "transparent",
          border: "1px solid var(--border)",
          borderRadius: "7px",
          padding: "6px 14px",
          color: "var(--muted)",
          fontFamily: "var(--mono)",
          fontSize: "11px",
          cursor: "pointer",
          letterSpacing: ".06em",
        }}
      >
        ⚙ 设置
      </button>
    </header>
  );
}
