// @ts-check
import { defineConfig, passthroughImageService } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  // A project site, so everything is served under /ytkew/.
  site: "https://dtdhruv.github.io",
  base: "/ytkew",
  trailingSlash: "always",

  // Nothing here needs resizing: the icon and wordmark are SVGs, and the
  // screenshot is served from public/ already sized. Skipping the default
  // service keeps sharp -- and libvips with it -- out of the tree.
  image: { service: passthroughImageService() },

  integrations: [
    starlight({
      title: "ytkew",
      description:
        "A terminal YouTube Music player in the spirit of kew: cover art, a spectrum visualizer, album-derived colours, vim keys and MPRIS, playing from your YouTube Music account.",
      logo: {
        src: "./src/assets/ytkew.svg",
        alt: "",
      },
      favicon: "/ytkew.svg",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/dtDhruv/ytkew",
        },
      ],
      editLink: {
        baseUrl: "https://github.com/dtDhruv/ytkew/edit/main/docs/",
      },
      customCss: ["./src/styles/ytkew.css"],
      lastUpdated: true,
      sidebar: [
        {
          label: "Getting started",
          items: [
            { label: "Introduction", slug: "start/introduction" },
            { label: "Install", slug: "start/install" },
            { label: "Signing in", slug: "start/signing-in" },
          ],
        },
        {
          label: "Using it",
          items: [
            { label: "The interface", slug: "guide/interface" },
            { label: "Keys", slug: "guide/keys" },
            { label: "Search", slug: "guide/search" },
            { label: "The queue", slug: "guide/queue" },
            { label: "Library", slug: "guide/library" },
          ],
        },
        {
          label: "Appearance",
          items: [
            { label: "Themes", slug: "look/themes" },
            { label: "Album art", slug: "look/album-art" },
            { label: "Layout", slug: "look/layout" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "Configuration", slug: "reference/configuration" },
            { label: "Desktop integration", slug: "reference/desktop" },
            { label: "Troubleshooting", slug: "reference/troubleshooting" },
            { label: "Architecture", slug: "reference/architecture" },
          ],
        },
      ],
    }),
  ],
});
