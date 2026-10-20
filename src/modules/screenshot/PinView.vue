<script setup lang="ts">
import { currentMonitor, getCurrentWindow, PhysicalSize } from "@tauri-apps/api/window";
import { onBeforeUnmount, onMounted, ref } from "vue";
import UiIcon from "../../components/UiIcon.vue";
import { assetUrl, call } from "../../lib/ipc";

const props = defineProps<{ imagePath: string }>();
const scale = ref(1);
const maxScale = ref(4);
const showControls = ref(false);
const resizing = ref(false);
const pinned = ref(true);
const start = ref({ x: 0, y: 0, width: 0, height: 0 });
const currentWindow = getCurrentWindow();
let imageRatio = 1;
let unlistenResize: (() => void) | null = null;

function pinImage(): HTMLImageElement | null {
  return document.querySelector<HTMLImageElement>(".pin-image");
}

async function updateScaleFromWindow() {
  const image = pinImage();
  if (!image?.naturalWidth || !image?.naturalHeight) return;
  const factor = await currentWindow.scaleFactor();
  const size = await currentWindow.innerSize();
  scale.value = Math.min(
    size.width / factor / image.naturalWidth,
    size.height / factor / image.naturalHeight,
  );
}

async function updateMaxScale(image: HTMLImageElement) {
  const monitor = await currentMonitor();
  if (!monitor) return;
  const availableWidth = monitor.size.width / monitor.scaleFactor - 40;
  const availableHeight = monitor.size.height / monitor.scaleFactor - 40;
  maxScale.value = Math.max(
    0.2,
    Math.min(availableWidth / image.naturalWidth, availableHeight / image.naturalHeight),
  );
}

async function setLogicalWindowSize(width: number, height: number) {
  const factor = await currentWindow.scaleFactor();
  await currentWindow.setSize(
    new PhysicalSize(
      Math.max(1, Math.round(width * factor)),
      Math.max(1, Math.round(height * factor)),
    ),
  );
}

async function setPinScale(next: number) {
  const image = pinImage();
  if (!image?.naturalWidth || !image?.naturalHeight) return;
  const clamped = Math.max(0.08, Math.min(maxScale.value, next));
  const width = Math.max(120, image.naturalWidth * clamped);
  const height = Math.max(80, image.naturalHeight * clamped);
  scale.value = clamped;
  await setLogicalWindowSize(width, height);
}

async function onImageLoad(event: Event) {
  const image = event.currentTarget as HTMLImageElement;
  imageRatio = image.naturalWidth / image.naturalHeight;
  await updateMaxScale(image);
  await updateScaleFromWindow();
}

function onWheel(event: WheelEvent) {
  event.preventDefault();
  const factor = event.deltaY > 0 ? 0.9 : 1.1;
  void setPinScale(scale.value * factor);
}

function finishResize(event: PointerEvent) {
  resizing.value = false;
  try {
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  } catch {
    // WebView 可能已提前释放指针捕获。
  }
}

function startResize(event: PointerEvent) {
  resizing.value = true;
  start.value = {
    x: event.clientX,
    y: event.clientY,
    width: window.innerWidth,
    height: window.innerHeight,
  };
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}

async function onResize(event: PointerEvent) {
  if (!resizing.value) return;
  const deltaX = event.clientX - start.value.x;
  const deltaY = event.clientY - start.value.y;
  let width: number;
  let height: number;
  if (Math.abs(deltaX) >= Math.abs(deltaY)) {
    width = start.value.width + deltaX;
    height = width / imageRatio;
  } else {
    height = start.value.height + deltaY;
    width = height * imageRatio;
  }
  width = Math.max(
    120,
    Math.min(maxScale.value * (pinImage()?.naturalWidth || width), width),
  );
  height = width / imageRatio;
  if (height < 80) {
    height = 80;
    width = height * imageRatio;
  }
  await setLogicalWindowSize(width, height);
}

async function copyImage() {
  await call("copy_history_image", { path: props.imagePath });
}

async function closePin() {
  await currentWindow.close();
}

async function togglePinned() {
  pinned.value = !pinned.value;
  await currentWindow.setAlwaysOnTop(pinned.value);
}

onMounted(async () => {
  window.addEventListener("wheel", onWheel, { passive: false });
  unlistenResize = await currentWindow.onResized(async ({ payload }) => {
    const image = pinImage();
    if (!image?.naturalWidth || !image?.naturalHeight) return;
    const factor = await currentWindow.scaleFactor();
    scale.value = Math.min(
      payload.width / factor / image.naturalWidth,
      payload.height / factor / image.naturalHeight,
    );
  });
});

onBeforeUnmount(() => {
  window.removeEventListener("wheel", onWheel);
  unlistenResize?.();
});
</script>

<template>
  <main
    class="pin-root"
    @mouseenter="showControls = true"
    @mouseleave="showControls = false"
  >
    <div class="drag-area" data-tauri-drag-region @dblclick="copyImage">
      <img class="pin-image" :src="assetUrl(imagePath)" alt="贴图" draggable="false" @load="onImageLoad" />
    </div>
    <div v-if="showControls" class="pin-controls">
      <button title="复制" @click.stop="copyImage"><UiIcon name="copy" :size="14" /></button>
      <button
        :class="{ active: pinned }"
        :title="pinned ? '取消置顶' : '置顶'"
        @click.stop="togglePinned"
      >
        <UiIcon name="pin" :size="14" />
      </button>
      <span>{{ Math.round(scale * 100) }}%</span>
      <button title="关闭" @click.stop="closePin"><UiIcon name="close" :size="14" /></button>
    </div>
    <div
      class="resize-corner"
      @pointerdown.stop.prevent="startResize"
      @pointermove.stop.prevent="onResize"
      @pointerup.stop="finishResize"
      @pointercancel.stop="finishResize"
    ></div>
  </main>
</template>

<style scoped>
.pin-root {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  border-radius: 9px;
  background: transparent;
  box-shadow: 0 14px 45px rgba(0, 0, 0, 0.32);
}

.drag-area {
  width: 100%;
  height: 100%;
  cursor: grab;
}

.drag-area:active {
  cursor: grabbing;
}

.pin-image {
  display: block;
  width: 100%;
  height: 100%;
  border-radius: 9px;
  object-fit: contain;
  user-select: none;
}

.pin-controls {
  position: absolute;
  top: 7px;
  right: 7px;
  display: flex;
  align-items: center;
  gap: 3px;
  padding: 4px;
  border: 1px solid rgba(255, 255, 255, 0.16);
  border-radius: 7px;
  background: rgba(22, 23, 26, 0.9);
  color: rgba(255, 255, 255, 0.75);
  backdrop-filter: blur(8px);
}

.pin-controls button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: inherit;
  cursor: pointer;
}

.pin-controls button:hover {
  background: rgba(255, 255, 255, 0.1);
}

.pin-controls button.active {
  color: #ffad4d;
  background: rgba(247, 147, 30, 0.14);
}

.pin-controls span {
  min-width: 34px;
  font-size: 8px;
  text-align: center;
}

.resize-corner {
  position: absolute;
  right: 0;
  bottom: 0;
  width: 17px;
  height: 17px;
  cursor: nwse-resize;
  background:
    linear-gradient(135deg, transparent 45%, rgba(255, 255, 255, 0.38) 46% 52%, transparent 53%),
    linear-gradient(135deg, transparent 65%, rgba(255, 255, 255, 0.28) 66% 72%, transparent 73%);
}
</style>
