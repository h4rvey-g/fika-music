import { onBeforeUnmount, readonly, ref, watch, type Ref } from "vue";

export type TrackListScrollBehavior = "auto" | "smooth";

type LocateTrack = (
  behavior: TrackListScrollBehavior,
  isCurrent: () => boolean,
) => boolean | Promise<boolean>;

type TrackListFollowOptions = {
  viewport: Ref<HTMLElement | null>;
  locate: LocateTrack;
  isActive?: () => boolean;
};

const SCROLL_KEYS = new Set([
  "ArrowDown",
  "ArrowUp",
  "End",
  "Home",
  "PageDown",
  "PageUp",
  " ",
]);

export function useTrackListFollow({
  viewport,
  locate,
  isActive = () => true,
}: TrackListFollowOptions) {
  const isFollowing = ref(true);
  let requestGeneration = 0;
  let attachedViewport: HTMLElement | null = null;
  let resizeObserver: ResizeObserver | null = null;

  function beginEntry() {
    requestGeneration += 1;
    isFollowing.value = true;
    cancelViewportAnimation(attachedViewport);
  }

  function cancelPending() {
    requestGeneration += 1;
    cancelViewportAnimation(attachedViewport);
  }

  async function locateEntry() {
    return requestLocate("auto");
  }

  async function followTrackChange() {
    const behavior = prefersReducedMotion() ? "auto" : "smooth";
    return requestLocate(behavior);
  }

  async function recenter() {
    return requestLocate("auto");
  }

  async function requestLocate(behavior: TrackListScrollBehavior) {
    if (!isFollowing.value || !isActive()) {
      return false;
    }
    const generation = ++requestGeneration;
    const isCurrent = () =>
      generation === requestGeneration && isFollowing.value && isActive();
    return locate(behavior, isCurrent);
  }

  function pauseFollowing() {
    if (!isFollowing.value || !isActive()) {
      return;
    }
    isFollowing.value = false;
    requestGeneration += 1;
    cancelViewportAnimation(attachedViewport);
  }

  function handleKeyboardIntent(event: KeyboardEvent) {
    if (!SCROLL_KEYS.has(event.key) || isTextEntryTarget(event.target)) {
      return;
    }
    pauseFollowing();
  }

  function handlePointerIntent(event: PointerEvent) {
    const element = attachedViewport;
    if (!element || event.target !== element) {
      return;
    }
    const nativeScrollbarWidth = Math.max(0, element.offsetWidth - element.clientWidth);
    const scrollbarHitWidth = nativeScrollbarWidth
      || (element.scrollHeight > element.clientHeight ? 12 : 0);
    if (!scrollbarHitWidth) {
      return;
    }
    const bounds = element.getBoundingClientRect();
    if (event.clientX >= bounds.right - scrollbarHitWidth) {
      pauseFollowing();
    }
  }

  function attach(element: HTMLElement | null) {
    detach();
    attachedViewport = element;
    if (!element) {
      return;
    }
    element.addEventListener("wheel", pauseFollowing, { capture: true, passive: true });
    element.addEventListener("touchmove", pauseFollowing, { capture: true, passive: true });
    element.addEventListener("keydown", handleKeyboardIntent, true);
    element.addEventListener("pointerdown", handlePointerIntent, true);
    if (typeof ResizeObserver !== "undefined") {
      resizeObserver = new ResizeObserver(() => {
        void recenter();
      });
      resizeObserver.observe(element);
    }
  }

  function detach() {
    if (attachedViewport) {
      attachedViewport.removeEventListener("wheel", pauseFollowing, true);
      attachedViewport.removeEventListener("touchmove", pauseFollowing, true);
      attachedViewport.removeEventListener("keydown", handleKeyboardIntent, true);
      attachedViewport.removeEventListener("pointerdown", handlePointerIntent, true);
    }
    resizeObserver?.disconnect();
    resizeObserver = null;
    attachedViewport = null;
  }

  watch(viewport, attach, { flush: "post", immediate: true });
  window.addEventListener("resize", recenter);

  onBeforeUnmount(() => {
    cancelPending();
    detach();
    window.removeEventListener("resize", recenter);
  });

  return {
    isFollowing: readonly(isFollowing),
    beginEntry,
    cancelPending,
    locateEntry,
    followTrackChange,
    recenter,
    pauseFollowing,
  };
}

export function centerElementInScrollViewport(
  viewport: HTMLElement,
  element: HTMLElement,
  behavior: TrackListScrollBehavior,
) {
  const viewportBounds = viewport.getBoundingClientRect();
  const elementBounds = element.getBoundingClientRect();
  const unclampedTop =
    viewport.scrollTop
    + elementBounds.top
    - viewportBounds.top
    - (viewport.clientHeight - elementBounds.height) / 2;
  const maxScrollTop = Math.max(0, viewport.scrollHeight - viewport.clientHeight);
  const top = Math.max(0, Math.min(unclampedTop, maxScrollTop));
  if (typeof viewport.scrollTo === "function") {
    viewport.scrollTo({
      top,
      left: viewport.scrollLeft,
      behavior,
    });
  } else {
    viewport.scrollTop = top;
  }
}

function cancelViewportAnimation(viewport: HTMLElement | null) {
  if (!viewport || typeof viewport.scrollTo !== "function") {
    return;
  }
  viewport.scrollTo({
    top: viewport.scrollTop,
    left: viewport.scrollLeft,
    behavior: "auto",
  });
}

function prefersReducedMotion() {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}

function isTextEntryTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  return Boolean(
    target.closest("input, textarea, select, button, [contenteditable='true']"),
  );
}
