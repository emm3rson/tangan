import type { ComponentType } from 'react'

export type ToolId = 'convert' | 'compress' | 'logo' | 'pdf-markdown' | 'video' | 'palette' | 'optimize-pdf'

export interface ToolDefinition {
  id: ToolId
  name: string
  description: string
  icon: ComponentType<{ size?: number }>
  component: ComponentType
}
