import { BatchWorkspace } from '../batch-workspace/BatchWorkspace'
import { ConvertIcon } from '@/components/ui/icons'
import type { ToolDefinition } from '../types'

export function ImageConverter() {
  return <BatchWorkspace mode="convert" />
}

export const imageConverterDefinition: ToolDefinition = {
  id: 'convert',
  name: 'Convert Images',
  description: 'Convert PNG, JPG, WebP, and SVG to PNG, JPG, or WebP.',
  icon: ConvertIcon,
  component: ImageConverter,
}
