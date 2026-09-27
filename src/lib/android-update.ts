import { gt, valid } from "semver";

const LATEST_RELEASE_API =
  "https://api.github.com/repos/h4rvey-g/fika-music/releases/latest";
const UPDATE_CHECK_TIMEOUT_MS = 30_000;

const ANDROID_ABI_BY_ARCHITECTURE: Readonly<Record<string, string>> = {
  aarch64: "arm64-v8a",
  arm: "armeabi-v7a",
  x86: "x86",
  x86_64: "x86_64",
};

type GitHubReleaseAsset = {
  name?: unknown;
  browser_download_url?: unknown;
};

export type AndroidUpdateRelease = {
  version: string;
  date: string | null;
  body: string | null;
  downloadUrl: string;
};

export type AndroidUpdateCheckOptions = {
  currentVersion: string;
  architecture: string;
  fetcher?: typeof fetch;
};

export async function checkAndroidUpdate(
  options: AndroidUpdateCheckOptions,
): Promise<AndroidUpdateRelease | null> {
  if (!valid(options.currentVersion)) {
    throw new Error("Unable to read the installed Android app version.");
  }

  const fetcher = options.fetcher ?? fetch;
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), UPDATE_CHECK_TIMEOUT_MS);
  let release: unknown;
  try {
    let response: Response;
    try {
      response = await fetcher(LATEST_RELEASE_API, {
        headers: { Accept: "application/vnd.github+json" },
        signal: controller.signal,
      });
    } catch {
      throw networkError(controller);
    }

    if (!response.ok) {
      if (response.status === 429 || (
        response.status === 403 && response.headers.get("x-ratelimit-remaining") === "0"
      )) {
        throw new Error("GitHub update checks are rate limited. Try again later.");
      }
      throw new Error(`Unable to check for Android updates (GitHub HTTP ${response.status}).`);
    }

    try {
      release = await response.json();
    } catch (error) {
      if (!controller.signal.aborted && error instanceof SyntaxError) {
        throw new Error("GitHub returned invalid Android update metadata.");
      }
      throw networkError(controller);
    }
  } finally {
    window.clearTimeout(timeout);
  }

  if (!isRecord(release)) {
    throw new Error("GitHub returned invalid Android update metadata.");
  }
  const version = releaseVersion(release.tag_name);
  if (!version) {
    throw new Error("GitHub returned invalid Android update metadata.");
  }
  if (!gt(version, options.currentVersion)) return null;

  const abi = ANDROID_ABI_BY_ARCHITECTURE[options.architecture];
  if (!abi || !Array.isArray(release.assets)) {
    throw new Error("No Android update package is available for this device.");
  }

  const expectedName = `Fika-Music_${version}_${abi}.apk`;
  const asset = release.assets.find(
    (candidate): candidate is GitHubReleaseAsset =>
      isRecord(candidate) && candidate.name === expectedName,
  );
  const downloadUrl = assetDownloadUrl(asset);
  if (!downloadUrl) {
    throw new Error("No Android update package is available for this device.");
  }

  return {
    version,
    date: typeof release.published_at === "string" ? release.published_at : null,
    body: typeof release.body === "string" ? release.body : null,
    downloadUrl,
  };
}

function networkError(controller: AbortController): Error {
  return new Error(controller.signal.aborted
    ? "Android update check timed out. Try again."
    : "Unable to reach GitHub for Android updates. Check your connection and try again.");
}

function releaseVersion(tagName: unknown): string | null {
  if (typeof tagName !== "string") return null;
  const version = tagName.startsWith("v") ? tagName.slice(1) : tagName;
  return valid(version);
}

function assetDownloadUrl(asset: GitHubReleaseAsset | undefined): string | null {
  if (typeof asset?.browser_download_url !== "string") return null;

  try {
    const url = new URL(asset.browser_download_url);
    if (url.protocol !== "https:" || url.hostname !== "github.com") return null;
    return url.toString();
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
