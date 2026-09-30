/** Last segment of a Windows or POSIX path. */
export function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** Drops the quotes Windows adds when copying a path ("Copy as path"). */
export function cleanPath(input: string): string {
  return input.trim().replace(/^"(.*)"$/, "$1");
}

/** File name without its last extension: `take1.mov` -> `take1`. */
export function stem(path: string): string {
  const name = fileName(path);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

/** Appends `.ext` unless the name already ends with it (dialogs do not always add it). */
export function withExtension(path: string, ext: string): string {
  return path.toLowerCase().endsWith(`.${ext.toLowerCase()}`) ? path : `${path}.${ext}`;
}
