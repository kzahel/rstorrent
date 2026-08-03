import { defineConfig } from 'astro/config'

export default defineConfig({
  site: 'https://rstorrent.com',
  output: 'static',
  build: {
    inlineStylesheets: 'never',
  },
})
