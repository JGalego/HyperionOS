import { ImageResponse } from "next/og";

export const size = { width: 64, height: 64 };
export const contentType = "image/png";

export default function Icon() {
  const accent = "#d9a54a";

  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "#0b0a08",
          borderRadius: 14,
        }}
      >
        <div style={{ position: "relative", width: 34, height: 40, display: "flex" }}>
          <div style={{ position: "absolute", left: 0, top: 0, width: 9, height: 40, background: accent }} />
          <div style={{ position: "absolute", left: 25, top: 0, width: 9, height: 40, background: accent }} />
          <div style={{ position: "absolute", left: 9, top: 9, width: 21, height: 9, background: accent }} />
        </div>
      </div>
    ),
    { ...size }
  );
}
