// Which grammar colours a file in the Files window: a Shiki language id (VS Code's TextMate
// grammars), told from the file's name, then its first line; `null` is plain text.

/** Whole names, lower case. */
const NAMES: Record<string, string> = {
  dockerfile: "dockerfile",
  containerfile: "dockerfile",
  makefile: "make",
  gnumakefile: "make",
  justfile: "just",
  "cmakelists.txt": "cmake",
  gemfile: "ruby",
  rakefile: "ruby",
  podfile: "ruby",
  vagrantfile: "ruby",
  brewfile: "ruby",
  artisan: "php",
  "composer.lock": "json",
  "cargo.lock": "toml",
  "pipfile.lock": "json",
  pipfile: "toml",
  ".editorconfig": "ini",
  ".npmrc": "ini",
  ".gitconfig": "ini",
  ".gitmodules": "ini",
  ".prettierrc": "json",
  ".eslintrc": "json",
  ".babelrc": "json",
  ".swcrc": "json",
  "tsconfig.json": "jsonc",
  "jsconfig.json": "jsonc",
  ".htaccess": "apache",
  "nginx.conf": "nginx",
  ".bashrc": "shellscript",
  ".bash_profile": "shellscript",
  ".profile": "shellscript",
  ".zshrc": "shellscript",
  ".zprofile": "shellscript",
  codeowners: "codeowners",
  "ssh_config": "ssh-config",
};

/** Endings of more than one part, longest first. */
const COMPOUND: Array<[string, string]> = [
  [".blade.php", "blade"],
  [".html.twig", "twig"],
  [".html.erb", "erb"],
  [".d.ts", "typescript"],
];

const EXTENSIONS: Record<string, string> = {
  js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "jsx",
  ts: "typescript", mts: "typescript", cts: "typescript", tsx: "tsx",
  vue: "vue", svelte: "svelte", astro: "astro",
  json: "json", jsonc: "jsonc", json5: "json5", jsonl: "jsonl", webmanifest: "json", ipynb: "json", map: "json",
  html: "html", htm: "html", xhtml: "html",
  css: "css", scss: "scss", sass: "sass", less: "less", styl: "stylus", pcss: "postcss", postcss: "postcss",
  md: "markdown", markdown: "markdown", mdx: "mdx",
  yml: "yaml", yaml: "yaml", toml: "toml", ini: "ini", cfg: "ini", properties: "properties",
  xml: "xml", svg: "xml", plist: "xml", xsd: "xml", xsl: "xsl", csproj: "xml",
  php: "php", phtml: "php",
  py: "python", pyi: "python", pyw: "python",
  rb: "ruby", rake: "ruby", gemspec: "ruby", erb: "erb",
  go: "go", rs: "rust", java: "java", kt: "kotlin", kts: "kotlin", groovy: "groovy", gradle: "groovy", scala: "scala",
  swift: "swift", m: "objective-c", mm: "objective-cpp",
  c: "c", h: "c", cc: "cpp", cpp: "cpp", cxx: "cpp", hpp: "cpp", hh: "cpp", hxx: "cpp",
  cs: "csharp", fs: "fsharp", fsx: "fsharp", vb: "vb", dart: "dart", lua: "lua",
  sh: "shellscript", bash: "shellscript", zsh: "shellscript", ksh: "shellscript", fish: "fish",
  ps1: "powershell", psm1: "powershell", bat: "bat", cmd: "bat",
  sql: "sql", graphql: "graphql", gql: "graphql", prisma: "prisma",
  twig: "twig", jinja: "jinja", jinja2: "jinja", j2: "jinja", liquid: "liquid", hbs: "handlebars", handlebars: "handlebars",
  pug: "pug", jade: "pug", haml: "haml", cshtml: "razor", razor: "razor",
  tf: "terraform", tfvars: "terraform", hcl: "hcl", nix: "nix", proto: "proto",
  ex: "elixir", exs: "elixir", erl: "erlang", hrl: "erlang", hs: "haskell", elm: "elm", ml: "ocaml", mli: "ocaml",
  clj: "clojure", cljs: "clojure", edn: "clojure", r: "r", jl: "julia", pl: "perl", pm: "perl",
  zig: "zig", nim: "nim", cr: "crystal", gleam: "gleam", coffee: "coffee", sol: "solidity",
  vim: "vim", diff: "diff", patch: "diff", log: "log", csv: "csv", tsv: "tsv",
  tex: "latex", rst: "rst", adoc: "asciidoc", http: "http",
  glsl: "glsl", frag: "glsl", vert: "glsl", wgsl: "wgsl", hlsl: "hlsl", asm: "asm", s: "asm",
  service: "systemd", timer: "systemd", cmake: "cmake",
};

/** Programs a script's first line can name (`#!/usr/bin/env node`). */
const SHEBANGS: Array<[RegExp, string]> = [
  [/\b(node|deno|bun)\b/, "javascript"],
  [/\bpython[\d.]*\b/, "python"],
  [/\b(bash|sh|zsh|dash|ksh)\b/, "shellscript"],
  [/\bruby\b/, "ruby"],
  [/\bphp\b/, "php"],
  [/\bperl\b/, "perl"],
  [/\bfish\b/, "fish"],
];

/** The grammar for `file` (a path, `/` between its parts) whose first line is `first`. */
export function languageOf(file: string, first = ""): string | null {
  const name = file.slice(file.lastIndexOf("/") + 1).toLowerCase();
  if (NAMES[name]) return NAMES[name];
  if (name === ".env" || name.startsWith(".env.") || name.endsWith(".env")) return "dotenv";
  if (name.startsWith("dockerfile.") || name.endsWith(".dockerfile")) return "dockerfile";
  for (const [ending, lang] of COMPOUND) if (name.endsWith(ending)) return lang;
  const dot = name.lastIndexOf(".");
  if (dot > 0 && EXTENSIONS[name.slice(dot + 1)]) return EXTENSIONS[name.slice(dot + 1)];
  if (first.startsWith("#!")) for (const [program, lang] of SHEBANGS) if (program.test(first)) return lang;
  return null;
}
