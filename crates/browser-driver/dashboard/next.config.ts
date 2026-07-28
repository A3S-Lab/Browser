import type { NextConfig } from "next";
import { PHASE_DEVELOPMENT_SERVER } from "next/constants";

const DAEMON_ORIGIN = process.env.DAEMON_URL || "http://localhost:4848";

const config = (phase: string): NextConfig => {
  const isDevelopment = phase === PHASE_DEVELOPMENT_SERVER;

  return {
    ...(isDevelopment ? {} : { output: "export" }),
    images: { unoptimized: true },
    devIndicators: false,
    env: {
      NEXT_PUBLIC_DAEMON_URL: DAEMON_ORIGIN,
    },
    ...(isDevelopment
      ? {
          async rewrites() {
            return [
              {
                source: "/api/:path*",
                destination: `${DAEMON_ORIGIN}/api/:path*`,
              },
            ];
          },
        }
      : {}),
  };
};

export default config;
