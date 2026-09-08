#!/usr/bin/env node
// Project the existing Specta-generated wire contract using TypeScript's type
// checker. There is no hand-maintained parallel catalogue or DTO schema.
import ts from 'typescript';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const root = fileURLToPath(new URL('..', import.meta.url));
const sourcePath = resolve(root, 'apps/synth_desktop/src/renderer/src/generated/protocol.ts');
const outputPath = resolve(root, 'apps/synth_desktop/src-tauri/src/contract/desktop_tools.json');
const program = ts.createProgram([sourcePath], { strictNullChecks: true, skipLibCheck: true, target: ts.ScriptTarget.ES2022 });
const checker = program.getTypeChecker();
const source = program.getSourceFile(sourcePath);
let commands;
function visit(node) {
  if (ts.isVariableDeclaration(node) && node.name.getText(source) === 'commands') commands = node.initializer;
  ts.forEachChild(node, visit);
}
visit(source);
if (!commands || !ts.isObjectLiteralExpression(commands)) throw new Error('Missing generated command registry');

function projection() {
  const definitions = {};
  const seen = new Map();
  const F = ts.TypeFlags;
  function schema(type) {
    if (type.flags & (F.Any | F.Unknown | F.TypeParameter)) return {};
    if (type.flags & (F.Null | F.Void | F.Undefined)) return { type: 'null' };
    if (type.isStringLiteral() || type.isNumberLiteral()) return { const: type.value };
    if (type.flags & F.BooleanLiteral) return { const: type.intrinsicName === 'true' };
    if (type.flags & F.StringLike) return { type: 'string' };
    if (type.flags & (F.NumberLike | F.BigIntLike)) return { type: 'number' };
    if (type.flags & F.Boolean) return { type: 'boolean' };
    if (type.flags & F.Never) return { not: {} };
    if (type.isUnion()) return { anyOf: type.types.filter(t => !(t.flags & F.Undefined)).map(schema) };
    if (type.isIntersection()) return { allOf: type.types.map(schema) };
    if (checker.isArrayType(type)) return { type: 'array', items: schema(checker.getTypeArguments(type)[0]) };
    if (checker.isTupleType(type)) {
      const items = checker.getTypeArguments(type).map(schema);
      return { type: 'array', prefixItems: items, minItems: items.length, maxItems: items.length };
    }
    if (!(type.flags & F.Object)) throw new Error(`Unsupported wire type ${checker.typeToString(type)}`);
    if (seen.has(type)) return { $ref: `#/$defs/${seen.get(type)}` };
    const name = `T${seen.size + 1}`;
    seen.set(type, name);
    const value = { type: 'object', properties: {} };
    definitions[name] = value;
    const required = [];
    for (const property of type.getProperties()) {
      const declaration = property.valueDeclaration ?? property.declarations?.[0] ?? source;
      value.properties[property.name] = schema(checker.getTypeOfSymbolAtLocation(property, declaration));
      if (!(property.flags & ts.SymbolFlags.Optional)) required.push(property.name);
    }
    if (required.length) value.required = required;
    const indexed = type.getStringIndexType();
    value.additionalProperties = indexed ? schema(indexed) : false;
    return { $ref: `#/$defs/${name}` };
  }
  return { schema, definitions };
}

const tools = commands.properties.map(property => {
  if (!ts.isPropertyAssignment(property) || !ts.isArrowFunction(property.initializer)) throw new Error('Unsupported command declaration');
  const fn = property.initializer;
  let operation;
  function find(node) {
    if (ts.isCallExpression(node) && node.expression.getText(source) === '__TAURI_INVOKE') {
      if (!ts.isStringLiteral(node.arguments[0])) throw new Error('Nonliteral command name');
      operation = node.arguments[0].text;
    }
    ts.forEachChild(node, find);
  }
  find(fn.body);
  if (!operation) throw new Error('Command does not invoke the registered native handler');
  const input = projection();
  const properties = {};
  const required = [];
  for (const parameter of fn.parameters) {
    const name = parameter.name.getText(source);
    const type = checker.getTypeAtLocation(parameter);
    properties[name] = input.schema(type);
    if (!parameter.questionToken && !(type.isUnion() && type.types.some(t => t.flags & ts.TypeFlags.Null))) required.push(name);
  }
  const output = projection();
  // typedError is a renderer-only convenience envelope. The native command
  // returns its success DTO directly and transport errors separately.
  const call = fn.body;
  if (!ts.isCallExpression(call) || !call.typeArguments?.length) throw new Error(`Missing wire result type: ${operation}`);
  const resultSchema = output.schema(checker.getTypeFromTypeNode(call.typeArguments[0]));
  return {
    name: operation,
    description: `Workshop ${operation.replaceAll('_', ' ')}. Uses the same typed native handler as the desktop. Explicitly human-controlled decisions require the desktop workflow.`,
    inputSchema: { type: 'object', properties, required, additionalProperties: false, $defs: input.definitions },
    outputSchema: { type: 'object', properties: { result: resultSchema }, required: ['result'], $defs: output.definitions },
    annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false, openWorldHint: true },
    _meta: { 'workshop/operationId': `desktop.${operation}.v1` }
  };
});
const generated = JSON.stringify({ tools }, null, 2) + '\n';
if (process.argv.includes('--check')) {
  if (readFileSync(outputPath, 'utf8') !== generated) throw new Error('Desktop tool schemas are stale; regenerate and review');
} else writeFileSync(outputPath, generated);
console.log(`${tools.length} desktop operation schemas ${process.argv.includes('--check') ? 'verified' : 'generated'}`);
