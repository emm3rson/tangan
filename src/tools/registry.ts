import { imageConverterDefinition } from './image-converter'
import { traceToSvgDefinition } from './trace-to-svg'
import { imageCompressorDefinition } from './image-compressor'
import { webLogoPackDefinition } from './web-logo-pack'
import { pdfToMarkdownDefinition } from './pdf-to-markdown'
import { videoProcessorDefinition } from './video-processor'
import { colorPaletteExtractorDefinition } from './color-palette-extractor'
import { pdfOptimizerDefinition } from './pdf-optimizer'
import type { ToolId } from './types'

export const tools = [
  imageConverterDefinition,
  traceToSvgDefinition,
  imageCompressorDefinition,
  webLogoPackDefinition,
  pdfToMarkdownDefinition,
  videoProcessorDefinition,
  colorPaletteExtractorDefinition,
  pdfOptimizerDefinition,
]

export const toolById = (id: ToolId) =>
  tools.find((tool) => tool.id === id)!
