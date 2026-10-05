export function nextDialogFocusIndex(
  currentIndex: number,
  focusableCount: number,
  reverse: boolean,
): number {
  if (focusableCount <= 0) {
    return -1;
  }
  if (currentIndex < 0) {
    return reverse ? focusableCount - 1 : 0;
  }
  return reverse
    ? (currentIndex - 1 + focusableCount) % focusableCount
    : (currentIndex + 1) % focusableCount;
}
