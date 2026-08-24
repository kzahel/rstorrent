export function createNativeUpdateCheckHandler(
  check: () => void,
): (generation: unknown) => boolean {
  let lastHandledGeneration = 0;
  return (generation) => {
    if (
      typeof generation !== "number" ||
      !Number.isSafeInteger(generation) ||
      generation <= lastHandledGeneration
    ) {
      return false;
    }
    lastHandledGeneration = generation;
    check();
    return true;
  };
}
