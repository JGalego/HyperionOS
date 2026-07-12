import { ImageResponse } from "next/og";
import { site } from "@/content/site";

export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

export default function Image() {
  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-start",
          justifyContent: "center",
          padding: "96px",
          background: "#0b0a08",
          color: "#f5f3ee",
          fontSize: 32,
        }}
      >
        <div style={{ position: "relative", width: 54, height: 64, display: "flex" }}>
          <div style={{ position: "absolute", left: 0, top: 0, width: 14, height: 64, background: "#d9a54a" }} />
          <div style={{ position: "absolute", left: 40, top: 0, width: 14, height: 64, background: "#d9a54a" }} />
          <div style={{ position: "absolute", left: 14, top: 14, width: 34, height: 14, background: "#d9a54a" }} />
        </div>
        <div style={{ display: "flex", marginTop: 48, fontSize: 76, fontWeight: 600 }}>
          {site.name}
        </div>
        <div style={{ display: "flex", marginTop: 20, color: "#a6a196", maxWidth: 820 }}>
          {site.tagline}
        </div>
      </div>
    ),
    { ...size }
  );
}
