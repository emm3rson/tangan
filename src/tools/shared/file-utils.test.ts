import { describe, expect, it } from 'vitest'
import { appendUniqueByPath, getErrorMessage } from './file-utils'

describe('appendUniqueByPath', () => {
  it('keeps the first item for each case-insensitive path across both lists', () => {
    const current = [{ path: 'C:\\images\\one.png', source: 'current' }]
    const incoming = [
      { path: 'c:\\IMAGES\\ONE.PNG', source: 'existing duplicate' },
      { path: 'C:\\images\\two.png', source: 'first new item' },
      { path: 'c:\\images\\TWO.PNG', source: 'new duplicate' }
    ]

    expect(appendUniqueByPath(current, incoming)).toEqual([
      current[0],
      incoming[1]
    ])
  })

  it('returns the current list when there are no additions', () => {
    const current = [{ path: 'C:\\images\\one.png' }]
    expect(appendUniqueByPath(current, [{ path: 'c:\\images\\ONE.PNG' }])).toBe(
      current
    )
  })
})

describe('getErrorMessage', () => {
  it('uses an error message when available and stringifies other thrown values', () => {
    expect(getErrorMessage(new Error('processing failed'))).toBe(
      'processing failed'
    )
    expect(getErrorMessage({ message: 404 })).toBe('404')
    expect(getErrorMessage('unknown failure')).toBe('unknown failure')
    expect(getErrorMessage(null)).toBe('null')
  })
})
