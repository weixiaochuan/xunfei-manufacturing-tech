import { useNavigate } from "react-router-dom";
import { AboutContent } from "@/components/about/AboutContent";

export default function AboutPage() {
  const navigate = useNavigate();

  return (
    <div className="anchor-page-layout">
      <div className="anchor-page-content" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <AboutContent
          showHeader
          showNavigateToSettings
          showSponsor
          showRecommend
          onNavigateSettings={() => navigate("/settings")}
        />
      </div>
    </div>
  );
}
