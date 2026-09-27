import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import NeteaseVerificationDialog from "./NeteaseVerificationDialog.vue";

const { openUrl } = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));

const verification = {
  message: "verification required",
  challenge: {
    url: "https://st.music.163.com/encrypt-pages?qrCode=fixture-only",
    qrImageDataUrl: "data:image/svg+xml;base64,PHN2Zy8+",
  },
};

describe("NeteaseVerificationDialog", () => {
  beforeEach(() => {
    openUrl.mockReset();
  });

  it("shows the real challenge without claiming that verification succeeded", async () => {
    const wrapper = mount(NeteaseVerificationDialog, { props: { verification } });
    expect(wrapper.get("img").attributes("src")).toBe(verification.challenge.qrImageDataUrl);
    expect(wrapper.text()).not.toContain("verified");
    expect(openUrl).not.toHaveBeenCalled();
    await wrapper.findAll("button").find((button) => button.text() === "Open verification")?.trigger("click");
    await flushPromises();
    expect(openUrl).toHaveBeenCalledWith(verification.challenge.url);
    expect(wrapper.emitted("retry")).toBeUndefined();
    wrapper.unmount();
  });

  it("preserves the challenge and reports failure to open its URL", async () => {
    openUrl.mockRejectedValue(new Error("No browser available"));
    const wrapper = mount(NeteaseVerificationDialog, { props: { verification } });
    await wrapper.findAll("button").find((button) => button.text() === "Open verification")?.trigger("click");
    await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toBe("No browser available");
    expect(wrapper.find("img").exists()).toBe(true);
    wrapper.unmount();
  });

  it("offers QR login instead of an invented challenge when parameters are missing", () => {
    const wrapper = mount(NeteaseVerificationDialog, {
      props: { verification: { message: "verification required", challenge: null } },
    });
    expect(wrapper.text()).toContain("No usable verification challenge was returned.");
    expect(wrapper.find("img").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("Open verification");
    wrapper.unmount();
  });

  it("closes on cancellation and retries only by explicit user action", async () => {
    const wrapper = mount(NeteaseVerificationDialog, { props: { verification } });
    await wrapper.get("dialog").trigger("cancel");
    expect(wrapper.emitted("close")).toHaveLength(1);
    await wrapper.findAll("button").find((button) => button.text() === "Retry")?.trigger("click");
    expect(wrapper.emitted("retry")).toHaveLength(1);
    wrapper.unmount();
  });
});
