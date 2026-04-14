interface LoadingScreenProps {
  title?: string;
  subtitle?: string;
}

export default function LoadingScreen({ title, subtitle }: LoadingScreenProps) {
  return (
    <div className="screen-cover" id="screen">
      <div className="spinner"></div>
      <div className="screen-title" id="screen-title">
        {title || "正在启动后端服务…"}
      </div>
      <div className="screen-sub" id="screen-sub">
        {subtitle || "请稍等"}
      </div>
    </div>
  );
}
