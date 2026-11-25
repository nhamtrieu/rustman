// Rustman script sandbox: `require()` and the libraries the Postman sandbox offers.
//
// Bundled libraries (crypto-js, lodash, moment, tv4, ajv, chai) come from Rust through
// globalThis.__lib(name) and are evaluated as CommonJS modules on first use. Small Node-style
// modules (uuid, buffer, querystring, util, xml2js) are implemented here.
// Native: globalThis.__random(n) -> array of n random bytes.
(function () {
  "use strict";

  // -------------------------------------------------------------------------
  // Web crypto subset (crypto-js and uuid need secure random numbers)
  // -------------------------------------------------------------------------

  if (!globalThis.crypto) {
    globalThis.crypto = {
      getRandomValues(arr) {
        if (!ArrayBuffer.isView(arr)) throw new TypeError("crypto.getRandomValues: expected a typed array");
        const bytes = globalThis.__random(arr.byteLength);
        new Uint8Array(arr.buffer, arr.byteOffset, arr.byteLength).set(bytes);
        return arr;
      },
    };
  }

  // -------------------------------------------------------------------------
  // UTF-8 and base64
  // -------------------------------------------------------------------------

  function utf8Encode(str) {
    const out = [];
    for (let i = 0; i < str.length; i++) {
      let c = str.charCodeAt(i);
      if (c >= 0xd800 && c < 0xdc00 && i + 1 < str.length) {
        const d = str.charCodeAt(i + 1);
        if (d >= 0xdc00 && d < 0xe000) {
          c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00);
          i++;
        } else c = 0xfffd;
      } else if (c >= 0xd800 && c < 0xe000) c = 0xfffd;
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
      else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    }
    return out;
  }

  function utf8Decode(bytes, start, end) {
    let s = "";
    let i = start;
    while (i < end) {
      const b = bytes[i];
      let c;
      let n = 0;
      if (b < 0x80) c = b;
      else if (b >= 0xc2 && b < 0xe0) (c = b & 31), (n = 1);
      else if (b >= 0xe0 && b < 0xf0) (c = b & 15), (n = 2);
      else if (b >= 0xf0 && b < 0xf5) (c = b & 7), (n = 3);
      else {
        s += "�";
        i++;
        continue;
      }
      // Truncated sequence at the end.
      if (n && i + n >= end) {
        s += "�";
        i++;
        continue;
      }
      let ok = true;
      for (let k = 1; k <= n; k++) {
        const x = bytes[i + k];
        if ((x & 0xc0) !== 0x80) {
          ok = false;
          break;
        }
        c = (c << 6) | (x & 63);
      }
      if (!ok) {
        s += "�";
        i++;
        continue;
      }
      i += n + 1;
      if (c >= 0x10000) {
        c -= 0x10000;
        s += String.fromCharCode(0xd800 + (c >> 10), 0xdc00 + (c & 1023));
      } else s += String.fromCharCode(c);
    }
    return s;
  }

  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  function base64Encode(bytes, start, end, url) {
    let out = "";
    for (let i = start; i < end; i += 3) {
      const a = bytes[i];
      const b = i + 1 < end ? bytes[i + 1] : 0;
      const c = i + 2 < end ? bytes[i + 2] : 0;
      const n = (a << 16) | (b << 8) | c;
      out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63] + (i + 1 < end ? B64[(n >> 6) & 63] : "=") + (i + 2 < end ? B64[n & 63] : "=");
    }
    return url ? out.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "") : out;
  }
  function base64Decode(str) {
    const s = String(str).replace(/-/g, "+").replace(/_/g, "/").replace(/[^A-Za-z0-9+/]/g, "");
    const out = [];
    let bits = 0;
    let acc = 0;
    for (const ch of s) {
      acc = (acc << 6) | B64.indexOf(ch);
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        out.push((acc >> bits) & 255);
      }
    }
    return out;
  }

  // -------------------------------------------------------------------------
  // buffer (the commonly used part of Node's Buffer)
  // -------------------------------------------------------------------------

  function normEnc(enc) {
    const e = String(enc || "utf8").toLowerCase().replace("-", "");
    if (e === "utf8" || e === "base64" || e === "base64url" || e === "hex" || e === "ascii") return e;
    if (e === "latin1" || e === "binary") return "latin1";
    if (e === "ucs2" || e === "utf16le") return "utf16le";
    throw new TypeError("Unknown encoding: " + enc);
  }

  function bytesOf(str, enc) {
    switch (normEnc(enc)) {
      case "utf8":
        return utf8Encode(str);
      case "base64":
      case "base64url":
        return base64Decode(str);
      case "hex": {
        const out = [];
        for (let i = 0; i + 1 < str.length; i += 2) {
          const v = parseInt(str.substr(i, 2), 16);
          if (Number.isNaN(v)) break;
          out.push(v);
        }
        return out;
      }
      case "utf16le": {
        const out = [];
        for (let i = 0; i < str.length; i++) {
          const c = str.charCodeAt(i);
          out.push(c & 255, c >> 8);
        }
        return out;
      }
      default: {
        const out = [];
        for (let i = 0; i < str.length; i++) out.push(str.charCodeAt(i) & 255);
        return out;
      }
    }
  }

  class Buffer extends Uint8Array {
    static from(value, encOrOffset, length) {
      if (typeof value === "string") return new Buffer(bytesOf(value, encOrOffset));
      if (value instanceof ArrayBuffer) return new Buffer(value, encOrOffset || 0, length === undefined ? value.byteLength - (encOrOffset || 0) : length);
      if (value && value.type === "Buffer" && Array.isArray(value.data)) return new Buffer(value.data);
      if (value && typeof value.length === "number") {
        const b = new Buffer(value.length);
        for (let i = 0; i < value.length; i++) b[i] = value[i] & 255;
        return b;
      }
      throw new TypeError("Buffer.from: unsupported argument");
    }
    static alloc(size, fill, enc) {
      const b = new Buffer(size);
      if (fill !== undefined) b.fill(typeof fill === "string" ? bytesOf(fill, enc)[0] || 0 : fill);
      return b;
    }
    static allocUnsafe(size) {
      return new Buffer(size);
    }
    static isBuffer(v) {
      return v instanceof Buffer;
    }
    static isEncoding(e) {
      try {
        normEnc(e);
        return true;
      } catch (_) {
        return false;
      }
    }
    static byteLength(v, enc) {
      return typeof v === "string" ? bytesOf(v, enc).length : v.byteLength;
    }
    static concat(list, total) {
      const len = total === undefined ? list.reduce((n, b) => n + b.length, 0) : total;
      const out = new Buffer(len);
      let pos = 0;
      for (const b of list) {
        if (pos >= len) break;
        out.set(b.length + pos > len ? b.subarray(0, len - pos) : b, pos);
        pos += b.length;
      }
      return out;
    }
    toString(enc, start, end) {
      const s = start === undefined ? 0 : Math.max(0, start);
      const e = end === undefined ? this.length : Math.min(this.length, end);
      switch (normEnc(enc)) {
        case "utf8":
          return utf8Decode(this, s, e);
        case "base64":
          return base64Encode(this, s, e, false);
        case "base64url":
          return base64Encode(this, s, e, true);
        case "hex": {
          let out = "";
          for (let i = s; i < e; i++) out += (this[i] < 16 ? "0" : "") + this[i].toString(16);
          return out;
        }
        case "utf16le": {
          let out = "";
          for (let i = s; i + 1 < e; i += 2) out += String.fromCharCode(this[i] | (this[i + 1] << 8));
          return out;
        }
        case "ascii": {
          let out = "";
          for (let i = s; i < e; i++) out += String.fromCharCode(this[i] & 127);
          return out;
        }
        default: {
          let out = "";
          for (let i = s; i < e; i++) out += String.fromCharCode(this[i]);
          return out;
        }
      }
    }
    toJSON() {
      return { type: "Buffer", data: Array.from(this) };
    }
    equals(other) {
      if (this.length !== other.length) return false;
      for (let i = 0; i < this.length; i++) if (this[i] !== other[i]) return false;
      return true;
    }
    slice(start, end) {
      return this.subarray(start, end);
    }
  }

  class TextEncoder {
    get encoding() {
      return "utf-8";
    }
    encode(s) {
      return new Uint8Array(utf8Encode(String(s === undefined ? "" : s)));
    }
  }
  class TextDecoder {
    get encoding() {
      return "utf-8";
    }
    decode(b) {
      if (!b) return "";
      const bytes = b instanceof ArrayBuffer ? new Uint8Array(b) : new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
      return utf8Decode(bytes, 0, bytes.length);
    }
  }
  if (!globalThis.TextEncoder) globalThis.TextEncoder = TextEncoder;
  if (!globalThis.TextDecoder) globalThis.TextDecoder = TextDecoder;

  // -------------------------------------------------------------------------
  // uuid (v4), querystring, util
  // -------------------------------------------------------------------------

  function uuidV4() {
    const b = globalThis.crypto.getRandomValues(new Uint8Array(16));
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = "";
    for (let i = 0; i < 16; i++) h += (b[i] < 16 ? "0" : "") + b[i].toString(16);
    return h.slice(0, 8) + "-" + h.slice(8, 12) + "-" + h.slice(12, 16) + "-" + h.slice(16, 20) + "-" + h.slice(20);
  }
  const uuid = Object.assign(() => uuidV4(), { v4: uuidV4 });

  const qsEscape = (s) => encodeURIComponent(s).replace(/[!'()*]/g, (c) => "%" + c.charCodeAt(0).toString(16).toUpperCase());
  const qsUnescape = (s) => {
    try {
      return decodeURIComponent(String(s).replace(/\+/g, " "));
    } catch (_) {
      return String(s);
    }
  };
  const querystring = {
    escape: qsEscape,
    unescape: qsUnescape,
    parse(str, sep, eq) {
      const out = {};
      if (typeof str !== "string" || str.length === 0) return out;
      for (const part of str.split(sep || "&")) {
        if (!part) continue;
        const i = part.indexOf(eq || "=");
        const k = qsUnescape(i < 0 ? part : part.slice(0, i));
        const v = i < 0 ? "" : qsUnescape(part.slice(i + 1));
        if (Object.prototype.hasOwnProperty.call(out, k)) out[k] = [].concat(out[k], v);
        else out[k] = v;
      }
      return out;
    },
    stringify(obj, sep, eq) {
      if (!obj || typeof obj !== "object") return "";
      const parts = [];
      for (const k of Object.keys(obj)) {
        const vals = Array.isArray(obj[k]) ? obj[k] : [obj[k]];
        for (const v of vals) parts.push(qsEscape(k) + (eq || "=") + qsEscape(v === undefined || v === null ? "" : typeof v === "object" ? "" : v));
      }
      return parts.join(sep || "&");
    },
  };
  querystring.decode = querystring.parse;
  querystring.encode = querystring.stringify;

  function inspect(v) {
    if (typeof v === "string") return "'" + v + "'";
    try {
      return typeof v === "object" && v !== null ? JSON.stringify(v) : String(v);
    } catch (_) {
      return String(v);
    }
  }
  const util = {
    format(fmt, ...args) {
      if (typeof fmt !== "string") return [fmt, ...args].map((a) => (typeof a === "string" ? a : inspect(a))).join(" ");
      let i = 0;
      let out = fmt.replace(/%[sdifjoO%]/g, (m) => {
        if (m === "%%") return "%";
        if (i >= args.length) return m;
        const a = args[i++];
        switch (m) {
          case "%s":
            return typeof a === "string" ? a : typeof a === "object" && a !== null ? inspect(a) : String(a);
          case "%d":
          case "%i":
            return String(m === "%i" ? parseInt(a, 10) : Number(a));
          case "%f":
            return String(parseFloat(a));
          case "%j":
            try {
              return JSON.stringify(a);
            } catch (_) {
              return "[Circular]";
            }
          default:
            return inspect(a);
        }
      });
      for (; i < args.length; i++) out += " " + (typeof args[i] === "string" ? args[i] : inspect(args[i]));
      return out;
    },
    inspect,
    isArray: Array.isArray,
    isString: (v) => typeof v === "string",
    isNumber: (v) => typeof v === "number",
    isObject: (v) => v !== null && typeof v === "object",
    isFunction: (v) => typeof v === "function",
    isUndefined: (v) => v === undefined,
    inherits(ctor, superCtor) {
      Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
      Object.setPrototypeOf(ctor, superCtor);
    },
    promisify(fn) {
      return (...args) => new Promise((resolve, reject) => fn(...args, (err, v) => (err ? reject(err) : resolve(v))));
    },
  };

  // -------------------------------------------------------------------------
  // xml2js (parseString with the common options) and Postman's xml2Json
  // -------------------------------------------------------------------------

  const ENTITIES = { lt: "<", gt: ">", amp: "&", quot: '"', apos: "'" };
  function decodeEntities(s) {
    return s.replace(/&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);/g, (m, e) => {
      if (e[0] === "#") {
        const code = e[1] === "x" || e[1] === "X" ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
        return Number.isNaN(code) ? m : String.fromCodePoint(code);
      }
      return Object.prototype.hasOwnProperty.call(ENTITIES, e) ? ENTITIES[e] : m;
    });
  }

  /// XML text -> { name, attrs, children: [node | { text, cdata }] } of the root element.
  function parseXml(xml) {
    const s = String(xml);
    let i = 0;
    let root = null;
    const stack = [];
    const fail = (msg) => {
      const line = s.slice(0, i).split("\n").length;
      throw new Error(msg + " (XML line " + line + ")");
    };
    const addChild = (node) => {
      if (stack.length) stack[stack.length - 1].children.push(node);
      else if (node.name) {
        if (root) fail("More than one root element");
        root = node;
      } else if (node.text.trim()) fail("Text outside of the root element");
    };
    while (i < s.length) {
      if (s[i] !== "<") {
        const j = s.indexOf("<", i);
        const end = j < 0 ? s.length : j;
        addChild({ text: decodeEntities(s.slice(i, end)) });
        i = end;
        continue;
      }
      if (s.startsWith("<!--", i)) {
        const j = s.indexOf("-->", i + 4);
        if (j < 0) fail("Unclosed comment");
        i = j + 3;
      } else if (s.startsWith("<![CDATA[", i)) {
        const j = s.indexOf("]]>", i + 9);
        if (j < 0) fail("Unclosed CDATA section");
        addChild({ text: s.slice(i + 9, j), cdata: true });
        i = j + 3;
      } else if (s.startsWith("<?", i)) {
        const j = s.indexOf("?>", i + 2);
        if (j < 0) fail("Unclosed processing instruction");
        i = j + 2;
      } else if (s.startsWith("<!", i)) {
        // DOCTYPE, possibly with an internal subset.
        let depth = 0;
        let j = i + 2;
        for (; j < s.length; j++) {
          if (s[j] === "[") depth++;
          else if (s[j] === "]") depth--;
          else if (s[j] === ">" && depth <= 0) break;
        }
        i = j + 1;
      } else if (s[i + 1] === "/") {
        const j = s.indexOf(">", i);
        if (j < 0) fail("Unclosed end tag");
        const name = s.slice(i + 2, j).trim();
        const open = stack.pop();
        if (!open || open.name !== name) fail("Unexpected end tag </" + name + ">" + (open ? ", expected </" + open.name + ">" : ""));
        addChild(open);
        i = j + 1;
      } else {
        const m = /^<([^\s/>]+)/.exec(s.slice(i, i + 256));
        if (!m) fail("Invalid tag");
        const node = { name: m[1], attrs: {}, children: [] };
        i += m[0].length;
        const attrRe = /\s*([^\s=/>]+)\s*=\s*("([^"]*)"|'([^']*)')|\s*(\/?)>/y;
        for (;;) {
          attrRe.lastIndex = i;
          const a = attrRe.exec(s);
          if (!a) fail("Invalid attribute in <" + node.name + ">");
          i = attrRe.lastIndex;
          if (a[1] !== undefined) {
            node.attrs[a[1]] = decodeEntities(a[3] !== undefined ? a[3] : a[4]);
            continue;
          }
          if (a[5] === "/") addChild(node);
          else stack.push(node);
          break;
        }
      }
    }
    if (stack.length) fail("Unclosed element <" + stack[stack.length - 1].name + ">");
    if (!root) fail("No root element");
    return root;
  }

  const XML_DEFAULTS = { explicitArray: true, explicitRoot: true, mergeAttrs: false, ignoreAttrs: false, attrkey: "$", charkey: "_", trim: false, normalize: false, explicitCharkey: false, emptyTag: "" };

  function toXml2js(node, o) {
    let text = "";
    let hasCdata = false;
    const obj = {};
    const add = (key, value) => {
      if (Object.prototype.hasOwnProperty.call(obj, key)) {
        if (!Array.isArray(obj[key]) || !o.explicitArray) obj[key] = [].concat(obj[key]);
        obj[key].push(value);
      } else obj[key] = o.explicitArray ? [value] : value;
    };
    const attrNames = Object.keys(node.attrs);
    if (attrNames.length && !o.ignoreAttrs) {
      if (o.mergeAttrs) for (const k of attrNames) add(k, node.attrs[k]);
      else obj[o.attrkey] = Object.assign({}, node.attrs);
    }
    let hasChildren = false;
    for (const c of node.children) {
      if (c.name) {
        hasChildren = true;
        add(c.name, toXml2js(c, o));
      } else {
        text += c.text;
        hasCdata = hasCdata || !!c.cdata;
      }
    }
    if (o.trim) text = text.trim();
    if (o.normalize) text = text.replace(/\s{2,}/g, " ").trim();
    const keys = Object.keys(obj);
    if (!keys.length && !hasChildren) return text === "" ? o.emptyTag : o.explicitCharkey ? { [o.charkey]: text } : text;
    if (text !== "" && (!/^\s*$/.test(text) || hasCdata)) obj[o.charkey] = text;
    return obj;
  }

  function xmlToObject(xml, options) {
    const o = Object.assign({}, XML_DEFAULTS, options || {});
    const root = parseXml(xml);
    const value = toXml2js(root, o);
    return o.explicitRoot ? { [root.name]: value } : value;
  }

  class Parser {
    constructor(options) {
      this.options = options || {};
    }
    parseString(xml, cb) {
      let result;
      try {
        result = xmlToObject(xml, this.options);
      } catch (e) {
        if (cb) return cb(e);
        throw e;
      }
      if (cb) cb(null, result);
      return result;
    }
    parseStringPromise(xml) {
      return new Promise((resolve, reject) => this.parseString(xml, (e, r) => (e ? reject(e) : resolve(r))));
    }
  }
  const xml2js = {
    Parser,
    defaults: { "0.2": XML_DEFAULTS, "0.1": XML_DEFAULTS },
    parseString(xml, options, cb) {
      if (typeof options === "function") return new Parser({}).parseString(xml, options);
      return new Parser(options).parseString(xml, cb);
    },
    parseStringPromise(xml, options) {
      return new Parser(options).parseStringPromise(xml);
    },
  };

  // -------------------------------------------------------------------------
  // require
  // -------------------------------------------------------------------------

  const BUILTIN = {
    uuid: () => uuid,
    buffer: () => ({ Buffer }),
    querystring: () => querystring,
    util: () => util,
    xml2js: () => xml2js,
    atob: () => globalThis.atob,
    btoa: () => globalThis.btoa,
  };
  const BUNDLED = ["crypto-js", "lodash", "moment", "tv4", "ajv", "chai"];
  const cache = Object.create(null);
  // Modules being evaluated (lodash reads `root._` while loading, which must not load it again).
  const loading = new Set();

  function load(name) {
    if (BUILTIN[name]) return BUILTIN[name]();
    const src = BUNDLED.indexOf(name) >= 0 ? globalThis.__lib(name) : undefined;
    if (typeof src !== "string") {
      const available = BUNDLED.concat(Object.keys(BUILTIN)).sort().join(", ");
      throw new Error("Cannot find module '" + name + "'. Modules available in Rustman scripts: " + available);
    }
    const module = { exports: {} };
    // CommonJS wrapper; `define` is hidden so UMD builds pick module.exports.
    new Function("module", "exports", "require", "define", src)(module, module.exports, globalThis.require, undefined);
    return module.exports;
  }

  globalThis.require = function require(name) {
    if (typeof name !== "string") throw new TypeError("require: the module name must be a string");
    const key = name.replace(/^node:/, "");
    if (!(key in cache)) {
      loading.add(key);
      try {
        cache[key] = load(key);
      } finally {
        loading.delete(key);
      }
    }
    return cache[key];
  };

  // Globals of the Postman sandbox, loaded on first use.
  const lazy = (name, module) => {
    Object.defineProperty(globalThis, name, {
      configurable: true,
      enumerable: false,
      get() {
        if (loading.has(module)) return undefined;
        const v = globalThis.require(module);
        Object.defineProperty(globalThis, name, { value: v, writable: true, configurable: true });
        return v;
      },
      set(v) {
        Object.defineProperty(globalThis, name, { value: v, writable: true, configurable: true });
      },
    });
  };
  lazy("CryptoJS", "crypto-js");
  lazy("_", "lodash");
  lazy("tv4", "tv4");
  globalThis.Buffer = Buffer;
  globalThis.xml2Json = function xml2Json(xml) {
    return xmlToObject(xml, { explicitArray: false, trim: true, mergeAttrs: false });
  };
})();
