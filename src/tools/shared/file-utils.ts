export function appendUniqueByPath<T extends { path: string }>(
  current: T[],
  incoming: T[]
): T[] {
  const paths = new Set(current.map((file) => file.path.toLowerCase()))
  const additions = incoming.filter((file) => {
    const path = file.path.toLowerCase()
    if (paths.has(path)) return false
    paths.add(path)
    return true
  })

  return additions.length === 0 ? current : [...current, ...additions]
}

export function getErrorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message)
  }
  return String(error)
}
