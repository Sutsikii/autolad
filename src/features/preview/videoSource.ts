const SEEK_TIMEOUT_MS = 3000;
const SAME_FRAME = 0.02;

/**
 * Makes `video` show `time` of the file at `url`, resolving once the frame is ready.
 * A broken file or a stalled seek resolves too (after a timeout): the caller keeps going.
 */
export function showSource(video: HTMLVideoElement, url: string, time: number): Promise<void> {
  return new Promise((resolve) => {
    const finish = () => {
      clearTimeout(timer);
      video.removeEventListener("seeked", finish);
      video.removeEventListener("error", finish);
      resolve();
    };
    const timer = setTimeout(finish, SEEK_TIMEOUT_MS);

    const seek = () => {
      if (Math.abs(video.currentTime - time) < SAME_FRAME) {
        finish();
        return;
      }
      video.addEventListener("seeked", finish, { once: true });
      video.currentTime = time;
    };

    video.addEventListener("error", finish, { once: true });
    if (video.dataset.src === url) {
      seek();
      return;
    }
    video.dataset.src = url;
    video.addEventListener("loadedmetadata", seek, { once: true });
    video.src = url;
  });
}
