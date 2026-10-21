// The Markdown preview's plugins that bring no types of their own (lib/markdownPreview).
declare module "markdown-it-task-lists" {
  import type { PluginWithOptions } from "markdown-it";
  const taskLists: PluginWithOptions<{ enabled?: boolean; label?: boolean; labelAfter?: boolean }>;
  export default taskLists;
}
