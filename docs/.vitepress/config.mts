import { defineConfig } from 'vitepress'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: 'bash-splitter',
  description: 'Split a bash command string into its individual commands.',
  base: '/bash-splitter/',
  cleanUrls: true,
  lastUpdated: true,
  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    nav: [
      { text: 'Guide', link: '/guide/' },
      { text: 'Reference', link: '/reference/coverage' },
      { text: 'Changelog', link: '/changelog' }
    ],

    sidebar: [
      {
        text: 'Guide',
        items: [{ text: 'Overview', link: '/guide/' }]
      },
      {
        text: 'Reference',
        items: [
          { text: 'Coverage', link: '/reference/coverage' },
          { text: 'Redirects', link: '/reference/redirects' }
        ]
      }
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/webspam/bash-splitter' }
    ],

    search: { provider: 'local' },

    editLink: {
      pattern: 'https://github.com/webspam/bash-splitter/edit/master/docs/:path'
    },

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Copyright © 2026-present webspam'
    }
  }
})
