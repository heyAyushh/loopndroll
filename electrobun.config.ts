import type { ElectrobunConfig } from "electrobun";
import { readFileSync } from "node:fs";

const releaseBaseUrl = process.env["RELEASE_BASE_URL"] || "";
const enableCodesign = process.env["ELECTROBUN_ENABLE_CODESIGN"] === "true";
const enableNotarize = process.env["ELECTROBUN_ENABLE_NOTARIZE"] === "true";
const packageVersion = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")).version;

export default {
  app: {
    name: "Loopndroll",
    identifier: "dev.loopndroll.app",
    version: packageVersion,
  },
  release: {
    baseUrl: releaseBaseUrl,
    generatePatch: false,
  },
  build: {
    bun: {
      entrypoint: "src/bun/index.ts",
    },
    copy: {
      "dist/index.html": "views/app/index.html",
      "dist/assets": "views/app/assets",
    },
    watchIgnore: ["dist/**"],
    mac: {
      codesign: enableCodesign,
      notarize: enableNotarize,
      bundleCEF: false,
      icons: "build/icon.iconset",
    },
    linux: {
      bundleCEF: false,
      icon: "build/icon.png",
    },
    win: {
      bundleCEF: false,
      icon: "build/icon.png",
    },
  },
} satisfies ElectrobunConfig;
