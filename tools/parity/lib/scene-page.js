// The in-page half of tools/parity/scene-export.mjs (roadmap WP 0.5). It is
// evaluated in the game page (a classic script, through the e2e harness) and
// installs window.__mpSceneExport, which turns live three.js objects into
// the .mrscene header, its binary chunks and a digest of the live scene.
//
// It reads and never changes the game: the one action with an effect is
// renderer.compile(), which builds any shader programs not built yet so the
// uniforms that onBeforeCompile patches add can be read back (three keeps
// them in renderer.properties). The format is crates/mp_scene/FORMAT.md.
(() => {
  const THREE = window.__THREE;
  if (!THREE) throw new Error('scene-page: window.__THREE is missing');

  // MaterialKind (SPEC 6.2): the tags the JS materials carry, and the kinds
  // the exporter gives built-in materials without a patch.
  const CUSTOM_KINDS = new Set([
    'Terrain', 'Asphalt', 'Shoulder', 'Markings', 'Sea', 'SkyDome', 'CarLight', 'PoliceGlow',
    'TriplanarRock', 'Sandstone', 'Stucco', 'Siding',
    'CityAtlas', 'CityFacade', 'StreetAtlas', 'StreetFacade', 'ContainerAtlas',
    'GlowPoints', 'FlickerPoints', 'GroundPool', 'Neon', 'AmbientProp',
    'TrafficStreams', 'SkyGlow', 'Surf', 'LighthouseBeam', 'Steam', 'FloodBeam', 'Reflector',
    'Particles', 'SkidMarks',
  ]);
  const BUILTIN_KINDS = {
    MeshStandardMaterial: 'Standard', MeshPhysicalMaterial: 'Physical', MeshLambertMaterial: 'Lambert',
    MeshBasicMaterial: 'Basic', LineBasicMaterial: 'Line', SpriteMaterial: 'Sprite', PointsMaterial: 'Points',
  };
  // three's ShaderLib entry per built-in type (WebGLPrograms' shaderIDs):
  // its uniforms are the ones a patch did not add.
  const SHADER_ID = {
    MeshStandardMaterial: 'physical', MeshPhysicalMaterial: 'physical', MeshLambertMaterial: 'lambert',
    MeshBasicMaterial: 'basic', LineBasicMaterial: 'basic', SpriteMaterial: 'sprite', PointsMaterial: 'points',
  };
  const NODE_TYPES = new Set([
    'Group', 'Object3D', 'Mesh', 'InstancedMesh', 'Points', 'LineSegments', 'Line', 'LineLoop', 'Sprite',
    'DirectionalLight', 'HemisphereLight', 'SpotLight', 'PointLight', 'AmbientLight',
  ]);

  const isPatched = (m) => Object.hasOwn(m, 'onBeforeCompile') && m.onBeforeCompile !== THREE.Material.prototype.onBeforeCompile;
  const isCustom = (m) => m.isShaderMaterial || m.isRawShaderMaterial || isPatched(m);

  const COMPONENT = (a) => {
    if (a instanceof Float32Array) return 'f32';
    if (a instanceof Float64Array) return 'f64';
    if (a instanceof Uint8Array || a instanceof Uint8ClampedArray) return 'u8';
    if (a instanceof Uint16Array) return 'u16';
    if (a instanceof Uint32Array) return 'u32';
    if (a instanceof Int8Array) return 'i8';
    if (a instanceof Int16Array) return 'i16';
    if (a instanceof Int32Array) return 'i32';
    throw new Error('scene-page: unsupported array type ' + a?.constructor?.name);
  };
  const bytesOf = (a) => new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
  const hex = (buf) => Array.from(new Uint8Array(buf), (b) => b.toString(16).padStart(2, '0')).join('');
  const sha256 = async (a) => hex(await crypto.subtle.digest('SHA-256', bytesOf(a)));

  // A number JSON can carry; Infinity and NaN as tagged strings.
  const num = (v) => (Number.isFinite(v) ? v : { num: String(v) });

  // Only the plain values of userData (no objects that may be cyclic).
  function plainData(ud) {
    const out = {};
    for (const [k, v] of Object.entries(ud || {})) {
      if (typeof v === 'number') out[k] = num(v);
      else if (typeof v === 'string' || typeof v === 'boolean' || v === null) out[k] = v;
      else if (Array.isArray(v) && v.length <= 64 && v.every((x) => typeof x === 'number' || typeof x === 'string' || typeof x === 'boolean')) out[k] = v.map((x) => (typeof x === 'number' ? num(x) : x));
    }
    return out;
  }

  // One export: a scene's tables and its binary chunks.
  class Exporter {
    constructor() {
      this.accessors = [];
      this.chunks = [];      // Uint8Array per accessor, at accessor.offset
      this.binLength = 0;
      this.accByArray = new Map();
      this.meshes = []; this.meshByGeo = new Map(); this.meshUse = [];
      this.materials = []; this.matByObj = new Map();
      this.textures = []; this.texByObj = new Map(); this.pixelsBySource = new Map();
      this.nodes = []; this.instances = []; this.lights = []; this.roots = [];
      this.live = { meshes: [], textures: [] }; // what the digest is computed from
    }

    // A typed array as an accessor; the same array object is stored once.
    accessor(array, itemSize, normalized = false) {
      const hit = this.accByArray.get(array);
      if (hit !== undefined) {
        const a = this.accessors[hit];
        if (a.item_size !== itemSize || a.normalized !== normalized) throw new Error('scene-page: an array is shared with different layouts');
        return hit;
      }
      const offset = Math.ceil(this.binLength / 8) * 8;
      const bytes = bytesOf(array);
      const acc = { offset, count: array.length / itemSize, item_size: itemSize, component: COMPONENT(array), normalized: !!normalized };
      if (!Number.isInteger(acc.count)) throw new Error('scene-page: array length is not a multiple of its item size');
      this.accessors.push(acc);
      this.chunks.push(bytes);
      this.binLength = offset + bytes.byteLength;
      this.accByArray.set(array, this.accessors.length - 1);
      return this.accessors.length - 1;
    }

    // An attribute's own elements, de-interleaved when it is interleaved.
    static attrArray(attr) {
      if (attr.isInterleavedBufferAttribute) {
        const src = attr.data.array, stride = attr.data.stride, n = attr.count, k = attr.itemSize;
        const out = new src.constructor(n * k);
        for (let i = 0; i < n; i++) for (let c = 0; c < k; c++) out[i * k + c] = src[i * stride + attr.offset + c];
        return out;
      }
      if (attr.isGLBufferAttribute) throw new Error('scene-page: GLBufferAttribute cannot be exported');
      return attr.array;
    }

    attribute(name, attr, deint) {
      const array = deint.get(attr) ?? Exporter.attrArray(attr);
      deint.set(attr, array);
      const out = { name, accessor: this.accessor(array, attr.itemSize, attr.normalized) };
      if (attr.isInstancedBufferAttribute || attr.isInstancedInterleavedBuffer || attr.data?.isInstancedInterleavedBuffer) {
        out.instanced = true;
        out.mesh_per_attribute = attr.meshPerAttribute ?? attr.data.meshPerAttribute;
      }
      return { out, array };
    }

    mesh(geo) {
      const hit = this.meshByGeo.get(geo);
      if (hit !== undefined) return hit;
      if (!geo.isBufferGeometry) throw new Error('scene-page: geometry is not a BufferGeometry');
      for (const [k, v] of Object.entries(geo.morphAttributes || {})) if (v && v.length) throw new Error('scene-page: morph attribute ' + k + ' is not supported');
      this.deint ??= new Map();
      const live = { attributes: [], index: null, position: null };
      const attributes = [];
      for (const [name, attr] of Object.entries(geo.attributes)) {
        const { out, array } = this.attribute(name, attr, this.deint);
        attributes.push(out);
        live.attributes.push({ name, array });
        if (name === 'position') live.position = { array, itemSize: attr.itemSize, count: attr.count };
      }
      const desc = {
        name: geo.name,
        attributes,
        index: geo.index ? this.accessor(geo.index.array, 1, false) : null,
        groups: geo.groups.map((g) => ({ start: g.start, count: num(g.count), material_index: g.materialIndex ?? 0 })),
        draw_range: { start: geo.drawRange.start, count: Number.isFinite(geo.drawRange.count) ? geo.drawRange.count : null },
        bounding_box: geo.boundingBox ? [...geo.boundingBox.min.toArray(), ...geo.boundingBox.max.toArray()].map(num) : null,
        bounding_sphere: geo.boundingSphere ? [...geo.boundingSphere.center.toArray(), geo.boundingSphere.radius].map(num) : null,
      };
      if (geo.index) live.index = geo.index.array;
      this.meshes.push(desc);
      this.live.meshes.push(live);
      this.meshUse.push(false);
      const i = this.meshes.length - 1;
      this.meshByGeo.set(geo, i);
      return i;
    }

    // Pixels of a texture's image as stored: canvas pixels top row first
    // (flip_y says whether the GPU upload flips them), data textures as
    // their array.
    pixels(t) {
      const img = t.image;
      if (!img) throw new Error(`scene-page: texture "${t.name}" has no image`);
      if (img.data && ArrayBuffer.isView(img.data)) {
        const channels = { [THREE.RGBAFormat]: 4, [THREE.RedFormat]: 1, [THREE.RGFormat]: 2, [THREE.AlphaFormat]: 1, [THREE.LuminanceFormat]: 1 }[t.format];
        if (!channels) throw new Error('scene-page: data texture format ' + t.format);
        return { source: 'data', width: img.width, height: img.height, channels, array: img.data };
      }
      let canvas = null, source = 'canvas', url = null;
      if ((typeof HTMLCanvasElement !== 'undefined' && img instanceof HTMLCanvasElement) || (typeof OffscreenCanvas !== 'undefined' && img instanceof OffscreenCanvas)) {
        canvas = img;
      } else if ((typeof HTMLImageElement !== 'undefined' && img instanceof HTMLImageElement) || (typeof ImageBitmap !== 'undefined' && img instanceof ImageBitmap)) {
        source = 'image';
        url = img.src ? new URL(img.src, document.baseURI).pathname.replace(/^\/+/, '') : null;
        canvas = document.createElement('canvas');
        canvas.width = img.width; canvas.height = img.height;
        canvas.getContext('2d').drawImage(img, 0, 0);
      } else {
        throw new Error(`scene-page: texture "${t.name}" has an image of type ${img.constructor?.name}`);
      }
      const ctx = canvas.getContext('2d');
      if (!ctx) throw new Error(`scene-page: texture "${t.name}": canvas has no 2d context`);
      const d = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      return { source, url, width: canvas.width, height: canvas.height, channels: 4, array: new Uint8Array(d.buffer, d.byteOffset, d.byteLength) };
    }

    texture(t) {
      const hit = this.texByObj.get(t);
      if (hit !== undefined) return hit;
      if (t.isCubeTexture || t.isRenderTargetTexture || t.isCompressedTexture || t.isData3DTexture || t.isDataArrayTexture) {
        throw new Error(`scene-page: texture "${t.name}" is of an unsupported kind`);
      }
      const key = t.source;
      let px = this.pixelsBySource.get(key);
      if (!px) {
        px = this.pixels(t);
        px.accessor = this.accessor(px.array, px.channels, false);
        this.pixelsBySource.set(key, px);
      }
      const desc = {
        name: t.name,
        source: px.source,
        url: px.url,
        width: px.width, height: px.height, channels: px.channels,
        pixels: px.accessor,
        format: t.format, type: t.type,
        flip_y: t.flipY,
        color_space: t.colorSpace,
        premultiply_alpha: t.premultiplyAlpha,
        unpack_alignment: t.unpackAlignment,
        generate_mipmaps: t.generateMipmaps,
        wrap_s: t.wrapS, wrap_t: t.wrapT,
        mag_filter: t.magFilter, min_filter: t.minFilter,
        anisotropy: t.anisotropy,
        offset: t.offset.toArray(), repeat: t.repeat.toArray(), rotation: t.rotation, center: t.center.toArray(),
        matrix_auto_update: t.matrixAutoUpdate,
        channel: t.channel,
      };
      this.textures.push(desc);
      this.live.textures.push(px);
      const i = this.textures.length - 1;
      this.texByObj.set(t, i);
      return i;
    }

    // Material parameters and uniform values as JSON.
    value(v) {
      if (v === null || v === undefined) return null;
      const t = typeof v;
      if (t === 'number') return num(v);
      if (t === 'boolean' || t === 'string') return v;
      if (t === 'function') return undefined;
      if (v.isTexture) return { texture: this.texture(v) };
      if (v.isColor) return { color: [v.r, v.g, v.b] };
      if (v.isVector2 || v.isVector3 || v.isVector4 || v.isQuaternion) return { vec: v.toArray().map(num) };
      if (v.isMatrix3 || v.isMatrix4) return { mat: [...v.elements].map(num) };
      if (v.isEuler) return { euler: [v.x, v.y, v.z, v.order] };
      if (v.isObject3D) return { object: v.name || v.type };
      if (ArrayBuffer.isView(v)) return { array: Array.from(v, num) };
      if (Array.isArray(v)) return v.map((x) => this.value(x) ?? null);
      if (t === 'object') {
        const out = {};
        for (const [k, x] of Object.entries(v)) { const c = this.value(x); if (c !== undefined) out[k] = c; }
        return out;
      }
      throw new Error('scene-page: cannot serialise a ' + t);
    }

    material(m, renderer) {
      const hit = this.matByObj.get(m);
      if (hit !== undefined) return hit;
      let kind;
      if (isCustom(m)) {
        kind = m.userData.kind;
        if (!kind) throw new Error(`scene-page: untagged custom material (${m.type} "${m.name}", program key ${m.customProgramCacheKey?.()}): give it userData.kind`);
        if (!CUSTOM_KINDS.has(kind)) throw new Error('scene-page: unknown MaterialKind tag ' + kind);
      } else {
        if (m.userData.kind) throw new Error(`scene-page: material tagged ${m.userData.kind} has no shader patch (a clone?)`);
        kind = BUILTIN_KINDS[m.type];
        if (!kind) throw new Error('scene-page: no MaterialKind for built-in ' + m.type);
      }
      const params = {};
      for (const key of Object.keys(m)) {
        if (['uuid', 'id', 'name', 'type', 'version', 'userData', '_listeners', 'uniforms', 'uniformsGroups', 'vertexShader', 'fragmentShader'].includes(key)) continue;
        if (/^is[A-Z]/.test(key)) continue;
        const v = this.value(m[key]);
        if (v !== undefined) params[key.replace(/^_/, '')] = v;
      }
      const desc = {
        kind,
        kind_opts: m.userData.kindOpts ? this.value(m.userData.kindOpts) : null,
        type: m.type,
        name: m.name,
        program_key: Object.hasOwn(m, 'customProgramCacheKey') ? m.customProgramCacheKey() : null,
        params,
        uniforms: null,
        shader: null,
      };
      if (m.isShaderMaterial) {
        desc.uniforms = {};
        for (const [k, u] of Object.entries(m.uniforms)) desc.uniforms[k] = this.value(u.value) ?? null;
        desc.shader = { vertex: m.vertexShader, fragment: m.fragmentShader };
      } else if (isPatched(m)) {
        // Uniforms the patch added: the compiled set minus three's own.
        const compiled = renderer.properties.get(m).uniforms;
        if (!compiled) throw new Error(`scene-page: ${kind} material was never compiled`);
        const base = THREE.ShaderLib[SHADER_ID[m.type]]?.uniforms;
        if (!base) throw new Error('scene-page: no ShaderLib entry for ' + m.type);
        desc.uniforms = {};
        for (const [k, u] of Object.entries(compiled)) if (!(k in base)) desc.uniforms[k] = this.value(u.value) ?? null;
      }
      this.materials.push(desc);
      const i = this.materials.length - 1;
      this.matByObj.set(m, i);
      return i;
    }

    light(o, node) {
      const L = {
        node,
        type: o.type,
        color: [o.color.r, o.color.g, o.color.b],
        intensity: o.intensity,
        cast_shadow: !!o.castShadow,
      };
      if (o.isHemisphereLight) L.ground_color = [o.groundColor.r, o.groundColor.g, o.groundColor.b];
      if (o.isSpotLight || o.isPointLight) { L.distance = o.distance; L.decay = o.decay; }
      if (o.isSpotLight) { L.angle = o.angle; L.penumbra = o.penumbra; }
      if (o.target) {
        const p = new THREE.Vector3().setFromMatrixPosition(o.target.matrixWorld);
        L.target = p.toArray();
      }
      if (o.castShadow && o.shadow) {
        const s = o.shadow, c = s.camera;
        L.shadow = {
          map_size: s.mapSize.toArray(), bias: s.bias, normal_bias: s.normalBias, radius: s.radius, blur_samples: s.blurSamples,
          camera: { type: c.type, near: c.near, far: c.far, left: c.left ?? null, right: c.right ?? null, top: c.top ?? null, bottom: c.bottom ?? null, fov: c.fov ?? null },
        };
      }
      this.lights.push(L);
      return this.lights.length - 1;
    }

    node(o, parent, renderer) {
      if (!NODE_TYPES.has(o.type)) throw new Error(`scene-page: object "${o.name}" of type ${o.type} is not supported`);
      if (o.isSkinnedMesh || o.isBatchedMesh || o.isLOD) throw new Error('scene-page: unsupported object ' + o.type);
      const n = {
        name: o.name,
        // three leaves an InstancedMesh's type as 'Mesh'; the file names it.
        type: o.isInstancedMesh ? 'InstancedMesh' : o.type,
        parent,
        children: [],
        matrix: [...o.matrix.elements].map(num),
        matrix_world: [...o.matrixWorld.elements].map(num),
        visible: o.visible,
        matrix_auto_update: o.matrixAutoUpdate,
        frustum_culled: o.frustumCulled,
        render_order: o.renderOrder,
        cast_shadow: o.castShadow,
        receive_shadow: o.receiveShadow,
        layers: o.layers.mask,
        user_data: plainData(o.userData),
      };
      this.nodes.push(n);
      const idx = this.nodes.length - 1;
      if (parent !== null) this.nodes[parent].children.push(idx);
      if (o.isMesh || o.isPoints || o.isLine || o.isSprite) {
        n.mesh = this.mesh(o.geometry);
        if (o.isMesh) this.meshUse[n.mesh] = true;
        const mats = Array.isArray(o.material) ? o.material : [o.material];
        n.materials = mats.map((m) => this.material(m, renderer));
        n.multi_material = Array.isArray(o.material);
      }
      if (o.isInstancedMesh) {
        if (o.morphTexture) throw new Error('scene-page: instanced morph targets are not supported');
        this.instances.push({
          node: idx,
          count: o.count,
          capacity: o.instanceMatrix.count,
          matrices: this.accessor(o.instanceMatrix.array, 16, false),
          colors: o.instanceColor ? this.accessor(o.instanceColor.array, 3, false) : null,
          bounding_sphere: o.boundingSphere ? [...o.boundingSphere.center.toArray(), o.boundingSphere.radius].map(num) : null,
        });
        n.instances = this.instances.length - 1;
      }
      if (o.isSprite) n.center = o.center.toArray();
      if (o.isLight) n.light = this.light(o, idx);
      for (const c of o.children) this.node(c, idx, renderer);
      return idx;
    }

    addRoot(o, renderer) { this.roots.push(this.node(o, null, renderer)); }

    // The digest of the live objects (crates/mp_scene computes the same from
    // the file): per mesh counts, bounds, area and centroid and a SHA-256 per
    // attribute; per texture a SHA-256 of its pixels; per drawable node its
    // kinds, textures, instances and world bounds.
    async digest(name) {
      const meshes = [];
      for (let i = 0; i < this.meshes.length; i++) {
        const L = this.live.meshes[i];
        const attributes = {};
        for (const a of L.attributes) attributes[a.name] = await sha256(a.array);
        const P = L.position;
        const d = {
          vertices: P ? P.count : 0,
          indices: L.index ? L.index.length : 0,
          bounds: P ? localBounds(P) : null,
          attributes,
          index: L.index ? await sha256(L.index) : null,
          area: null, centroid: null,
        };
        if (this.meshUse[i] && P && P.itemSize === 3) Object.assign(d, areaCentroid(P.array, L.index, P.count));
        meshes.push(d);
      }
      const textures = [];
      for (const px of this.live.textures) textures.push({ width: px.width, height: px.height, channels: px.channels, sha256: await sha256(px.array) });
      const materials = this.materials.map((m) => ({ kind: m.kind, type: m.type, textures: texRefs(m) }));
      const drawables = [];
      for (let i = 0; i < this.nodes.length; i++) {
        const n = this.nodes[i];
        if (n.mesh === undefined) continue;
        const inst = n.instances !== undefined ? this.instances[n.instances] : null;
        const textures = [...new Set(n.materials.flatMap((m) => materials[m].textures))];
        const P = this.live.meshes[n.mesh].position;
        const im = inst ? this.chunkArray(inst.matrices) : null;
        drawables.push({
          node: i,
          type: n.type,
          mesh: n.mesh,
          kinds: n.materials.map((m) => this.materials[m].kind),
          textures,
          instances: inst ? inst.count : null,
          instance_matrices: inst ? await sha256(im) : null,
          instance_colors: inst && inst.colors !== null ? await sha256(this.chunkArray(inst.colors)) : null,
          world_bounds: P && P.itemSize === 3 ? worldBounds(P, n.matrix_world, im, inst ? inst.count : 0) : null,
        });
      }
      const kinds = {};
      for (const m of this.materials) kinds[m.kind] = (kinds[m.kind] || 0) + 1;
      return {
        version: 1,
        name,
        counts: {
          nodes: this.nodes.length, meshes: this.meshes.length, materials: this.materials.length, textures: this.textures.length,
          instances: this.instances.length, lights: this.lights.length, drawables: drawables.length,
          vertices: meshes.reduce((s, m) => s + m.vertices, 0), indices: meshes.reduce((s, m) => s + m.indices, 0),
          binary_bytes: this.binLength,
        },
        kinds,
        meshes, textures, materials, drawables,
      };
    }

    chunkArray(acc) {
      const a = this.accessors[acc];
      const b = this.chunks[acc];
      if (a.component !== 'f32') throw new Error('scene-page: expected f32');
      return new Float32Array(b.buffer, b.byteOffset, b.byteLength / 4);
    }

    // The binary blob: every chunk at its offset, zero padding between.
    blob() {
      const parts = [];
      let at = 0;
      for (let i = 0; i < this.chunks.length; i++) {
        const off = this.accessors[i].offset;
        if (off > at) parts.push(new Uint8Array(off - at));
        parts.push(this.chunks[i]);
        at = off + this.chunks[i].byteLength;
      }
      return new Blob(parts);
    }

    header(meta, extra) {
      return {
        format: 'mrscene', version: 1, meta,
        binary_length: this.binLength,
        accessors: this.accessors,
        meshes: this.meshes, instances: this.instances, materials: this.materials, textures: this.textures,
        nodes: this.nodes, roots: this.roots, lights: this.lights,
        night_params: extra.night_params ?? [], environment: extra.environment ?? null,
      };
    }
  }

  // Textures a material's parameters and uniforms refer to, in order.
  function texRefs(m) {
    const out = [];
    const walk = (v) => {
      if (!v || typeof v !== 'object') return;
      if (typeof v.texture === 'number' && Object.keys(v).length === 1) { if (!out.includes(v.texture)) out.push(v.texture); return; }
      for (const x of Object.values(v)) walk(x);
    };
    walk(m.params); walk(m.uniforms);
    return out;
  }

  function localBounds(P) {
    const a = P.array, k = P.itemSize, n = P.count;
    if (n === 0) return null;
    const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
    const dims = Math.min(3, k);
    for (let i = 0; i < n; i++) {
      for (let c = 0; c < dims; c++) {
        const v = a[i * k + c];
        if (v < lo[c]) lo[c] = v;
        if (v > hi[c]) hi[c] = v;
      }
    }
    return [...lo.slice(0, dims), ...hi.slice(0, dims)].map(num);
  }

  // Surface area and area-weighted centroid of the triangle list (indexed or
  // not; a trailing partial triangle is ignored). The arithmetic is spelled
  // out so the Rust side can repeat it exactly.
  function areaCentroid(a, index, count) {
    const n = index ? index.length : count;
    const tris = Math.floor(n / 3);
    let area = 0, cx = 0, cy = 0, cz = 0;
    for (let t = 0; t < tris; t++) {
      const i0 = (index ? index[t * 3] : t * 3) * 3, i1 = (index ? index[t * 3 + 1] : t * 3 + 1) * 3, i2 = (index ? index[t * 3 + 2] : t * 3 + 2) * 3;
      const ax = a[i0], ay = a[i0 + 1], az = a[i0 + 2];
      const ux = a[i1] - ax, uy = a[i1 + 1] - ay, uz = a[i1 + 2] - az;
      const vx = a[i2] - ax, vy = a[i2 + 1] - ay, vz = a[i2 + 2] - az;
      const nx = uy * vz - uz * vy, ny = uz * vx - ux * vz, nz = ux * vy - uy * vx;
      const ar = 0.5 * Math.sqrt(nx * nx + ny * ny + nz * nz);
      area += ar;
      cx += ar * ((ax + a[i1] + a[i2]) / 3);
      cy += ar * ((ay + a[i1 + 1] + a[i2 + 1]) / 3);
      cz += ar * ((az + a[i1 + 2] + a[i2 + 2]) / 3);
    }
    return { area, centroid: area > 0 ? [cx / area, cy / area, cz / area] : null };
  }

  // World-space bounds of every vertex: matrix_world × (instance matrix ×) p,
  // affine, evaluated left to right.
  function worldBounds(P, e, im, count) {
    const a = P.array, n = P.count;
    if (n === 0) return null;
    const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
    const put = (x, y, z) => {
      const wx = e[0] * x + e[4] * y + e[8] * z + e[12];
      const wy = e[1] * x + e[5] * y + e[9] * z + e[13];
      const wz = e[2] * x + e[6] * y + e[10] * z + e[14];
      if (wx < lo[0]) lo[0] = wx; if (wx > hi[0]) hi[0] = wx;
      if (wy < lo[1]) lo[1] = wy; if (wy > hi[1]) hi[1] = wy;
      if (wz < lo[2]) lo[2] = wz; if (wz > hi[2]) hi[2] = wz;
    };
    if (im) {
      if (count === 0) return null;
      for (let k = 0; k < count; k++) {
        const m = im.subarray(k * 16, k * 16 + 16);
        for (let i = 0; i < n; i++) {
          const x = a[i * 3], y = a[i * 3 + 1], z = a[i * 3 + 2];
          put(m[0] * x + m[4] * y + m[8] * z + m[12], m[1] * x + m[5] * y + m[9] * z + m[13], m[2] * x + m[6] * y + m[10] * z + m[14]);
        }
      }
    } else {
      for (let i = 0; i < n; i++) put(a[i * 3], a[i * 3 + 1], a[i * 3 + 2]);
    }
    return [...lo, ...hi].map(num);
  }

  const nextFrames = (k) => new Promise((resolve) => {
    const step = () => (k-- <= 0 ? resolve() : requestAnimationFrame(step));
    requestAnimationFrame(step);
  });

  function environment(world, camera, renderer) {
    const scene = world.realScene;
    const fog = scene.fog;
    return {
      fog: fog ? { type: fog.isFogExp2 ? 'FogExp2' : 'Fog', color: [fog.color.r, fog.color.g, fog.color.b], density: fog.density ?? null, near: fog.near ?? null, far: fog.far ?? null } : null,
      tone_mapping: renderer.toneMapping,
      tone_mapping_exposure: renderer.toneMappingExposure,
      environment_intensity: scene.environmentIntensity,
      night: world.sky.night,
      camera: { position: camera.position.toArray(), quaternion: camera.quaternion.toArray(), fov: camera.fov, near: camera.near, far: camera.far, aspect: camera.aspect },
    };
  }

  let current = null; // { header, digest, blob }

  async function finish(ex, name, meta, extra) {
    const digest = await ex.digest(name);
    const header = ex.header(meta, extra);
    current = { blob: ex.blob() };
    return { header: JSON.stringify(header), digest: JSON.stringify(digest), binary_length: ex.binLength };
  }

  // The level's world: world.root plus the sky's dome and lights.
  // base: terrain, road and sky only.
  async function exportWorld({ base = false, meta = {} } = {}) {
    const world = window.__world, camera = window.__camera, renderer = world.renderer;
    await nextFrames(3);
    renderer.compile(world.realScene, camera);
    const ex = new Exporter();
    const skyRoots = [world.sky.dome, world.sky.sun, world.sky.sun.target, world.sky.hemi];
    if (base) {
      const terrain = world.root.children.filter((c) => c.name === 'terrain');
      if (terrain.length !== 1 || !world.road?.group) throw new Error('scene-page: terrain or road group not found');
      for (const o of [...terrain, world.road.group]) ex.addRoot(o, renderer);
    } else {
      ex.addRoot(world.root, renderer);
    }
    for (const o of skyRoots) ex.addRoot(o, renderer);
    // Night parameters of the materials drawn. Some registered ones belong
    // to no mesh (the road's chevron material on a level without chevrons).
    const night_params = [];
    for (const nm of world.nightMaterials) {
      const i = ex.matByObj.get(nm.material);
      if (i !== undefined) night_params.push({ material: i, prop: nm.prop, day: nm.day, night: nm.night });
    }
    return finish(ex, meta.name, meta, { night_params, environment: environment(world, camera, renderer) });
  }

  // Every car model (each kind at each detail level the game uses, the far
  // model, the police liveries), the effects and the pursuit props, outside
  // the scene. These modules are the ones the game loaded (same URLs).
  async function exportModels({ meta = {} } = {}) {
    const world = window.__world, camera = window.__camera, renderer = world.renderer;
    const url = (p) => new URL(p, document.baseURI).href;
    const CarModel = await import(url('src/vehicles/CarModel.js'));
    const { Effects } = await import(url('src/game/Effects.js'));
    const { sawhorseModel, spikeStrip } = await import(url('src/game/PursuitView.js'));
    const root = new THREE.Group();
    root.name = 'models';
    const models = [];
    const add = (name, kind, opts) => {
      const h = CarModel.buildVehicle(kind, opts);
      const g = new THREE.Group();
      g.name = name;
      g.add(h.root);
      root.add(g);
      models.push({ name, kind, opts });
      return h;
    };
    for (const kind of CarModel.VEHICLE_KINDS) {
      add(`car:${kind}:high`, kind, { lod: 'high', seed: 0 });
      add(`car:${kind}:low`, kind, { lod: 'low', far: true, seed: 0 });
    }
    for (const kind of ['muscle', 'sports']) {
      add(`car:${kind}:police:high`, kind, { lod: 'high', livery: 'police', seed: 0 });
      add(`car:${kind}:police:low`, kind, { lod: 'low', livery: 'police', far: true, seed: 0 });
    }
    // Effects on a car of their own: smoke, sparks, skid marks, a headlight
    // pool, and nitro flames on its exhausts.
    const fx = new THREE.Group();
    fx.name = 'effects';
    root.add(fx);
    const effects = new Effects(fx, renderer, camera);
    const fxCar = CarModel.buildVehicle('sports', { lod: 'high', seed: 0 });
    fxCar.root.name = 'effects-car';
    fx.add(fxCar.root);
    effects.addCar({ model: fxCar });
    const props = new THREE.Group();
    props.name = 'pursuit-props';
    root.add(props);
    const saw = sawhorseModel();
    saw.root.name = 'sawhorse';
    props.add(saw.root);
    const spikes = spikeStrip(world.track, world.track.startS + 200, -4, 4);
    spikes.name = 'spike-strip';
    props.add(spikes);
    root.updateMatrixWorld(true);
    renderer.compile(root, camera, world.realScene);
    const ex = new Exporter();
    ex.addRoot(root, renderer);
    return finish(ex, meta.name, { ...meta, models }, {});
  }

  // A slice of the current export's binary, as base64.
  function readBinary(start, end) {
    return new Promise((resolve, reject) => {
      const r = new FileReader();
      r.onload = () => resolve(String(r.result).replace(/^data:[^,]*,/, ''));
      r.onerror = () => reject(r.error);
      r.readAsDataURL(current.blob.slice(start, end));
    });
  }

  // Every array and plain value on world.track after the world is built
  // (scenery sets runout and the like), in its insertion order.
  function trackDump() {
    const t = window.__world.track;
    const arrays = [], scalars = {}, skipped = [];
    const parts = [];
    let at = 0;
    for (const key of Object.keys(t)) {
      const v = t[key];
      if (ArrayBuffer.isView(v)) {
        const offset = Math.ceil(at / 8) * 8;
        if (offset > at) parts.push(new Uint8Array(offset - at));
        parts.push(bytesOf(v));
        arrays.push({ name: key, component: COMPONENT(v), length: v.length, offset, byte_length: v.byteLength });
        at = offset + v.byteLength;
      } else if (typeof v === 'number') scalars[key] = num(v);
      else if (typeof v === 'boolean' || typeof v === 'string' || v === null) scalars[key] = v;
      else if (typeof v === 'function' || key === 'level' || v instanceof Map) skipped.push(key);
      else {
        try { scalars[key] = JSON.parse(JSON.stringify(v, (k, x) => (typeof x === 'number' && !Number.isFinite(x) ? { num: String(x) } : x))); } catch { skipped.push(key); }
      }
    }
    current = { blob: new Blob(parts) };
    return { arrays, scalars, skipped, binary_length: at };
  }

  // Terrain heights at deterministic points (SPEC 5.4): `along` points on
  // and beside the road (s spread evenly, lateral offset from a golden-ratio
  // sequence over ±60 m), the rest over the terrain's bounds (Halton 2, 3).
  // The points are stored with the heights, so the Rust side need not
  // regenerate them.
  function terrainDump({ total = 10000, along = 6000 } = {}) {
    const w = window.__world, t = w.track, T = w.terrain;
    const xz = new Float64Array(total * 2), h = new Float64Array(total);
    const halton = (i, b) => { let f = 1, r = 0; while (i > 0) { f /= b; r += f * (i % b); i = Math.floor(i / b); } return r; };
    const p = {};
    for (let i = 0; i < total; i++) {
      let x, z;
      if (i < along) {
        const s = ((i + 0.5) / along) * t.length;
        const g = (i * 0.6180339887498949) % 1;
        t.pointAt(s, -60 + 120 * g, p);
        x = p.x; z = p.z;
      } else {
        const k = i - along + 1;
        x = T.minX + (T.maxX - T.minX) * halton(k, 2);
        z = T.minZ + (T.maxZ - T.minZ) * halton(k, 3);
      }
      xz[i * 2] = x; xz[i * 2 + 1] = z;
      h[i] = T.heightAt(x, z);
    }
    current = { blob: new Blob([bytesOf(xz), bytesOf(h)]) };
    return {
      count: total, along, bounds: [T.minX, T.minZ, T.maxX, T.maxZ],
      arrays: [
        { name: 'xz', component: 'f64', length: xz.length, offset: 0, byte_length: xz.byteLength },
        { name: 'height', component: 'f64', length: h.length, offset: xz.byteLength, byte_length: h.byteLength },
      ],
      binary_length: xz.byteLength + h.byteLength,
    };
  }

  window.__mpSceneExport = { exportWorld, exportModels, readBinary, trackDump, terrainDump, CUSTOM_KINDS: [...CUSTOM_KINDS], BUILTIN_KINDS };
})();
