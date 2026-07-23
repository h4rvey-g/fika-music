import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SourceRequest, SourceRequestOutcome } from "./plugin-api";
import {
  dispatchPluginRequest,
  importLxJsSource,
  importLxJsSourceUrl,
  selectLxJsSource,
  setPluginCapabilities,
} from "./plugin-api";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

describe("Plugin API", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("dispatches a typed SourceRequest through the Plugin command", async () => {
    const request: SourceRequest = {
      action: "musicSearch",
      source: "wy",
      keyword: "fika",
      page: 1,
      pageSize: 20,
    };
    const outcome: SourceRequestOutcome = {
      response: {
        action: "musicSearch",
        data: { isEnd: true, total: 0, list: [] },
      },
      diagnostics: [],
    };
    invokeMock.mockResolvedValue(outcome);

    await expect(
      dispatchPluginRequest("fika.runtime-demo", request, "request-1"),
    ).resolves.toEqual(outcome);
    expect(invokeMock).toHaveBeenCalledWith("dispatch_plugin_request", {
      pluginId: "fika.runtime-demo",
      request,
      requestId: "request-1",
    });
  });

  it("uses camelCase arguments for capability review", async () => {
    invokeMock.mockResolvedValue({ id: "fika.runtime-demo" });

    await setPluginCapabilities("fika.runtime-demo", ["network:any"], true);

    expect(invokeMock).toHaveBeenCalledWith("set_plugin_capabilities", {
      pluginId: "fika.runtime-demo",
      capabilities: ["network:any"],
      reviewed: true,
    });
  });

  it("selects and imports an LX JavaScript source", async () => {
    invokeMock
      .mockResolvedValueOnce("/downloads/source.js")
      .mockResolvedValueOnce({ id: "imported-lx-source" });

    await expect(selectLxJsSource()).resolves.toBe("/downloads/source.js");
    await importLxJsSource("/downloads/source.js");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "select_lx_js_source");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "import_lx_js_source", {
      sourcePath: "/downloads/source.js",
    });
  });

  it("imports an LX JavaScript source from a URL", async () => {
    invokeMock.mockResolvedValue({ id: "imported-lx-source" });

    await importLxJsSourceUrl("https://example.com/source.js");

    expect(invokeMock).toHaveBeenCalledWith("import_lx_js_source_url", {
      sourceUrl: "https://example.com/source.js",
    });
  });
});
