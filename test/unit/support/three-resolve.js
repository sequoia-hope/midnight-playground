// Resolve hook for ./three.js: the bare 'three' and 'three/addons/…'
// specifiers point at vendor/three, as in index.html's import map.
const VENDOR = new URL('../../../vendor/three/', import.meta.url);

export async function resolve(specifier, context, next) {
  if (specifier === 'three') return { url: new URL('build/three.module.js', VENDOR).href, shortCircuit: true };
  if (specifier.startsWith('three/addons/')) return { url: new URL('addons/' + specifier.slice('three/addons/'.length), VENDOR).href, shortCircuit: true };
  return next(specifier, context);
}
