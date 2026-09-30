/** Last segment of a Windows or POSIX path. */
export function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** Drops the quotes Windows adds when copying a path ("Copy as path"). */
export function cleanPath(input: string): string {
  return input.trim().replace(/^"(.*)"$/, "$1");
}
