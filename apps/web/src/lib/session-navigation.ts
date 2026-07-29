export type SessionNavigationDirection = -1 | 1;

export function sessionNeighborIndex(
  currentIndex: number,
  total: number,
  direction: SessionNavigationDirection,
): number | undefined {
  if (currentIndex < 0 || currentIndex >= total) {
    return undefined;
  }
  const nextIndex = currentIndex + direction;
  return nextIndex >= 0 && nextIndex < total ? nextIndex : undefined;
}
