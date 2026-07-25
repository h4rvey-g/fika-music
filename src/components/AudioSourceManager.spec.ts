import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AudioSourceManager from "./AudioSourceManager.vue";
import type { AudioSourceRecord } from "../lib/audio-source-api";
import { createAudioSourceRecord } from "../test/fixtures";

const apiMocks = vi.hoisted(() => ({
  clearAudioSourceDiagnostics: vi.fn(),
  importAudioSource: vi.fn(),
  importAudioSourceUrl: vi.fn(),
  listAudioSources: vi.fn(),
  refreshAudioSources: vi.fn(),
  removeAudioSource: vi.fn(),
  selectAudioSourceFile: vi.fn(),
  setAudioSourceCapabilities: vi.fn(),
  setAudioSourceEnabled: vi.fn(),
}));

vi.mock("../lib/audio-source-api", () => apiMocks);

function managerAudioSourceRecord(
  overrides: Partial<AudioSourceRecord> = {},
): AudioSourceRecord {
  return createAudioSourceRecord({
    description: "Audio source manager fixture",
    author: "Fika Tests",
    state: "needs-review",
    enabled: false,
    permissionsReviewed: false,
    grantedCapabilities: [],
    canEnable: false,
    ...overrides,
  });
}

describe("AudioSourceManager", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    apiMocks.listAudioSources.mockResolvedValue([managerAudioSourceRecord()]);
  });

  it("loads standalone audio source records", async () => {
    const wrapper = mount(AudioSourceManager);
    await flushPromises();

    expect(apiMocks.listAudioSources).toHaveBeenCalledOnce();
    expect(wrapper.text()).toContain("Imported Source");
    expect(wrapper.text()).toContain("Review required");
    expect(wrapper.emitted("sourcesChanged")?.[0]).toEqual([[managerAudioSourceRecord()]]);
    wrapper.unmount();
  });

  it("imports a local source without using Plugin APIs", async () => {
    const imported = managerAudioSourceRecord({ id: "new-source", name: "New Source" });
    apiMocks.selectAudioSourceFile.mockResolvedValue("/downloads/source.js");
    apiMocks.importAudioSource.mockResolvedValue(imported);
    const wrapper = mount(AudioSourceManager);
    await flushPromises();

    const importButton = wrapper
      .findAll("button")
      .find((button) => button.text().includes("Import file"));
    await importButton?.trigger("click");
    await flushPromises();

    expect(apiMocks.selectAudioSourceFile).toHaveBeenCalledOnce();
    expect(apiMocks.importAudioSource).toHaveBeenCalledWith("/downloads/source.js");
    expect(wrapper.text()).toContain("New Source imported");
    const changes = wrapper.emitted("sourcesChanged") ?? [];
    expect(changes[changes.length - 1]).toEqual([[managerAudioSourceRecord(), imported]]);
    wrapper.unmount();
  });

  it("reviews permissions before enabling a source", async () => {
    const selected = managerAudioSourceRecord({
      grantedCapabilities: ["network:any"],
    });
    const reviewed = managerAudioSourceRecord({
      state: "disabled",
      permissionsReviewed: true,
      grantedCapabilities: ["network:any"],
      canEnable: true,
    });
    const enabled = managerAudioSourceRecord({
      state: "enabled",
      enabled: true,
      permissionsReviewed: true,
      grantedCapabilities: ["network:any"],
      canEnable: true,
    });
    apiMocks.setAudioSourceCapabilities
      .mockResolvedValueOnce(selected)
      .mockResolvedValueOnce(reviewed);
    apiMocks.setAudioSourceEnabled.mockResolvedValue(enabled);
    const wrapper = mount(AudioSourceManager);
    await flushPromises();

    await wrapper.get('button[aria-label="Inspect Imported Source"]').trigger("click");
    const grant = wrapper.get<HTMLInputElement>('input[aria-label="Grant Any network host"]');
    await grant.setValue(true);
    await flushPromises();
    expect(apiMocks.setAudioSourceCapabilities).toHaveBeenCalledWith(
      "imported-source",
      ["network:any"],
      false,
    );

    const confirm = wrapper
      .findAll("button")
      .find((button) => button.text().includes("Confirm review"));
    await confirm?.trigger("click");
    await flushPromises();
    expect(apiMocks.setAudioSourceCapabilities).toHaveBeenLastCalledWith(
      "imported-source",
      ["network:any"],
      true,
    );

    await wrapper.get<HTMLInputElement>('input[aria-label="Enable Imported Source"]').setValue(true);
    await flushPromises();
    expect(apiMocks.setAudioSourceEnabled).toHaveBeenCalledWith("imported-source", true);
    wrapper.unmount();
  });
});
