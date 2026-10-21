// The Files window's syntax colours, as two TextMate themes (the form VS Code and Shiki read),
// one per theme of the app. Each colour is at least 4.5:1 on its ground (--bg-log).

type Rule = { scope: string[]; settings: { foreground?: string; fontStyle?: string } };
export type CodeTheme = { name: string; type: "dark" | "light"; colors: Record<string, string>; tokenColors: Rule[] };

type Palette = { text: string; comment: string; keyword: string; string: string; number: string; fn: string; type: string; property: string; punctuation: string; tag: string; attribute: string; namespace: string; invalid: string; inserted: string; deleted: string };

function theme(name: string, type: "dark" | "light", ground: string, p: Palette): CodeTheme {
  const rule = (scope: string[], foreground: string, fontStyle?: string): Rule => ({ scope, settings: fontStyle ? { foreground, fontStyle } : { foreground } });
  return {
    name,
    type,
    colors: { "editor.background": ground, "editor.foreground": p.text },
    tokenColors: [
      rule(["comment", "punctuation.definition.comment", "string.comment"], p.comment, "italic"),
      rule(["keyword", "storage", "storage.type", "storage.modifier", "keyword.control", "keyword.other.use", "keyword.other.namespace", "variable.language.this", "variable.language.self"], p.keyword),
      rule(["keyword.operator", "punctuation", "meta.brace", "punctuation.separator", "punctuation.terminator"], p.punctuation),
      rule(["string", "string.quoted", "string.template", "punctuation.definition.string", "markup.inline.raw"], p.string),
      rule(["constant.numeric", "constant.language", "constant.character", "constant.other", "support.constant", "string.regexp", "keyword.other.unit"], p.number),
      rule(["entity.name.function", "support.function", "meta.function-call entity.name.function", "variable.function", "keyword.other.special-method"], p.fn),
      rule(["entity.name.type", "entity.name.class", "support.class", "support.type", "entity.other.inherited-class", "storage.type.class", "entity.name.function.decorator", "meta.decorator"], p.type),
      rule(["variable", "variable.other", "meta.definition.variable"], p.text),
      rule(["variable.other.property", "variable.other.object.property", "support.type.property-name", "meta.object-literal.key", "entity.name.tag.yaml", "support.variable.property", "variable.other.constant.property"], p.property),
      rule(["entity.name.tag", "punctuation.definition.tag", "support.class.component"], p.tag),
      rule(["entity.other.attribute-name", "entity.other.attribute-name.class.css", "entity.other.attribute-name.id.css"], p.attribute),
      rule(["entity.name.namespace", "support.other.namespace", "entity.name.type.namespace", "storage.modifier.import", "storage.modifier.package"], p.namespace),
      rule(["variable.other.env", "variable.other.dotenv", "variable.other.key"], p.property),
      rule(["markup.heading", "entity.name.section"], p.fn, "bold"),
      rule(["markup.bold"], p.text, "bold"),
      rule(["markup.italic"], p.text, "italic"),
      rule(["markup.underline.link", "string.other.link"], p.fn),
      rule(["markup.inserted", "meta.diff.header.to-file"], p.inserted),
      rule(["markup.deleted", "meta.diff.header.from-file"], p.deleted),
      rule(["invalid", "invalid.illegal"], p.invalid),
    ],
  };
}

export const darkCode = theme("pitwall-dark", "dark", "#0b0c0f", {
  text: "#d4d7dc",
  comment: "#7d838d",
  keyword: "#c4a7e7",
  string: "#b5d99c",
  number: "#f0b27a",
  fn: "#8fc1e3",
  type: "#e6c384",
  property: "#a9c4d8",
  punctuation: "#a3a9b3",
  tag: "#86b8e8",
  attribute: "#d9b98c",
  namespace: "#c7ccd3",
  invalid: "#f28b8b",
  inserted: "#9ad7b0",
  deleted: "#f28b8b",
});

export const lightCode = theme("pitwall-light", "light", "#ffffff", {
  text: "#1b1e24",
  comment: "#5f6773",
  keyword: "#7a3eb8",
  string: "#1a6b32",
  number: "#a14d00",
  fn: "#0a55a8",
  type: "#8a4a00",
  property: "#1f5f7f",
  punctuation: "#4f5661",
  tag: "#1a6b32",
  attribute: "#0a55a8",
  namespace: "#343a43",
  invalid: "#b42a2a",
  inserted: "#15803d",
  deleted: "#b42a2a",
});
