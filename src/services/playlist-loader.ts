export async function loadAllPages<T>(
  firstPage: T[],
  loadNextPage: () => Promise<T[]>,
  key: (item: T) => string,
): Promise<T[]> {
  const result = [...firstPage];
  const seen = new Set(result.map(key));

  while (true) {
    const page = await loadNextPage();
    if (!page.length) return result;

    const fresh = page.filter((item) => !seen.has(key(item)));
    if (!fresh.length) return result;

    fresh.forEach((item) => seen.add(key(item)));
    result.push(...fresh);
  }
}
