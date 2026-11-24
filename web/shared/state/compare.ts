import { createStore } from '../lib/store'

export interface CompareCard {
  index: number
  kind: 'diff' | 'original only' | 'cleaned only'
  url: string
  name: string
  size: number
}

export type CompareSummary =
  | { state: 'running' }
  | { state: 'ok'; main: string; pages: number; ms: number }
  | { state: 'differs'; main: string; differing: number; pages: number; ms: number }
  | { state: 'failed'; message: string }

export interface CompareState {
  summary: CompareSummary | null
  cards: CompareCard[]
}

export const compareStore = createStore<CompareState>({ summary: null, cards: [] })

export function resetCompare() {
  for (const card of compareStore.get().cards) URL.revokeObjectURL(card.url)
  compareStore.set({ summary: null, cards: [] })
}
