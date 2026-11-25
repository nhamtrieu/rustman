// Rustman script sandbox: a Postman-compatible subset of the `pm` API.
//
// Input:  globalThis.__input, a JSON string set by Rust (scopes, request, response, info).
// Native: globalThis.__send(json) -> json, performs pm.sendRequest synchronously.
// Output: globalThis.__finish() returns a JSON string (variable ops, tests, logs, ...).
(function () {
  "use strict";

  const input = JSON.parse(globalThis.__input);
  const scopes = input.scopes;
  const out = { ops: [], tests: [], events: [], request: null, nextRequest: null, skip: false, error: null };
  const hasOwn = (o, k) => o != null && Object.prototype.hasOwnProperty.call(o, k);

  // -------------------------------------------------------------------------
  // Formatting helpers
  // -------------------------------------------------------------------------

  function inspect(v) {
    if (typeof v === "string") return "'" + v + "'";
    if (v === undefined) return "undefined";
    if (typeof v === "function") return "[Function" + (v.name ? ": " + v.name : "") + "]";
    if (v instanceof RegExp) return String(v);
    if (v instanceof Error) return v.name + ": " + v.message;
    if (v && typeof v === "object") {
      if (v.__isResponse) return "response";
      try {
        const s = JSON.stringify(v);
        return s.length > 300 ? s.slice(0, 297) + "..." : s;
      } catch (e) {
        return String(v);
      }
    }
    return String(v);
  }

  function logText(v) {
    if (typeof v === "string") return v;
    if (v instanceof Error) return v.stack ? v.name + ": " + v.message + "\n" + v.stack : v.name + ": " + v.message;
    if (v && typeof v === "object") {
      if (v.__isResponse) return "Response " + v.code + " " + v.status;
      try {
        return JSON.stringify(v, null, 2);
      } catch (e) {
        return String(v);
      }
    }
    return String(v);
  }

  // Line in the user script ("script" file) where an error was raised.
  function scriptLine(e) {
    const m = e && typeof e.stack === "string" ? /\(?script:(\d+)/.exec(e.stack) : null;
    return m ? Number(m[1]) : null;
  }

  function errMsg(e) {
    if (e && typeof e === "object" && "message" in e) {
      const name = e.name || "Error";
      const line = scriptLine(e);
      return name + ": " + e.message + (line ? " (line " + line + ")" : "");
    }
    return String(e);
  }

  // Values are stored as strings, like the variables of the workspace.
  function stringify(v) {
    if (typeof v === "string") return v;
    if (v === undefined || v === null) return "";
    if (typeof v === "object") {
      try {
        return JSON.stringify(v);
      } catch (e) {
        return String(v);
      }
    }
    return String(v);
  }

  // -------------------------------------------------------------------------
  // console
  // -------------------------------------------------------------------------

  const log = (level) => (...args) => {
    out.events.push({ kind: "log", level, text: args.map(logText).join(" ") });
  };
  globalThis.console = {
    log: log("log"),
    info: log("info"),
    warn: log("warn"),
    error: log("error"),
    debug: log("log"),
    trace: log("log"),
  };

  // -------------------------------------------------------------------------
  // Variables
  // -------------------------------------------------------------------------

  const dynamicVars = {
    $guid: uuid,
    $randomUUID: uuid,
    $timestamp: () => String(Math.floor(Date.now() / 1000)),
    $isoTimestamp: () => new Date().toISOString(),
    $randomInt: () => String(Math.floor(Math.random() * 1001)),
    $randomBoolean: () => String(Math.random() < 0.5),
    $randomEmail: () => "user" + Math.floor(Math.random() * 100000) + "@example.com",
  };

  function uuid() {
    const h = "0123456789abcdef";
    let s = "";
    for (let i = 0; i < 36; i++) {
      if (i === 8 || i === 13 || i === 18 || i === 23) s += "-";
      else if (i === 14) s += "4";
      else if (i === 19) s += h[8 + Math.floor(Math.random() * 4)];
      else s += h[Math.floor(Math.random() * 16)];
    }
    return s;
  }

  // Highest precedence first, like Postman.
  function lookup(key) {
    for (const store of [scopes.local, scopes.data, scopes.environment, scopes.collection, scopes.globals]) {
      if (hasOwn(store, key)) return store[key];
    }
    return undefined;
  }

  function replaceIn(template) {
    let s = String(template);
    for (let depth = 0; depth < 5 && s.indexOf("{{") !== -1; depth++) {
      let changed = false;
      s = s.replace(/\{\{([^{}]*)\}\}/g, (whole, raw) => {
        const name = raw.trim();
        const v = lookup(name);
        if (v !== undefined) {
          changed = true;
          return v;
        }
        if (hasOwn(dynamicVars, name)) {
          changed = true;
          return dynamicVars[name]();
        }
        return whole;
      });
      if (!changed) break;
    }
    return s;
  }

  function makeScope(name, label, writable) {
    const store = () => scopes[name];
    const missing = (what) => {
      console.warn("No environment is selected: pm.environment." + what + " was ignored.");
    };
    const api = {
      get(key) {
        const s = store();
        return hasOwn(s, key) ? s[key] : undefined;
      },
      has(key) {
        return hasOwn(store(), key);
      },
      toObject() {
        return Object.assign({}, store() || {});
      },
      replaceIn,
    };
    if (writable) {
      api.set = function (key, value) {
        const s = store();
        if (!s) return missing("set('" + key + "')");
        const v = stringify(value);
        s[String(key)] = v;
        out.ops.push({ op: "set", scope: name, key: String(key), value: v });
      };
      api.unset = function (key) {
        const s = store();
        if (!s) return missing("unset('" + key + "')");
        delete s[String(key)];
        out.ops.push({ op: "unset", scope: name, key: String(key) });
      };
      api.clear = function () {
        const s = store();
        if (!s) return missing("clear()");
        for (const k of Object.keys(s)) delete s[k];
        out.ops.push({ op: "clear", scope: name });
      };
    } else {
      api.set = api.unset = api.clear = function () {
        throw new TypeError(label + " is read-only");
      };
    }
    return api;
  }

  const environment = makeScope("environment", "pm.environment", true);
  Object.defineProperty(environment, "name", { value: input.environmentName || undefined, enumerable: true });

  const variables = {
    get: lookup,
    has: (key) => lookup(key) !== undefined,
    set(key, value) {
      const v = stringify(value);
      scopes.local[String(key)] = v;
      out.ops.push({ op: "set", scope: "local", key: String(key), value: v });
    },
    unset(key) {
      delete scopes.local[String(key)];
      out.ops.push({ op: "unset", scope: "local", key: String(key) });
    },
    toObject() {
      return Object.assign({}, scopes.globals, scopes.collection, scopes.environment || {}, scopes.data, scopes.local);
    },
    replaceIn,
  };

  // -------------------------------------------------------------------------
  // Headers (a Postman PropertyList-like view over [{key, value, disabled}])
  // -------------------------------------------------------------------------

  function parseHeader(h, value) {
    if (typeof h === "string") {
      if (value !== undefined) return { key: h, value: stringify(value) };
      const i = h.indexOf(":");
      return i === -1 ? { key: h.trim(), value: "" } : { key: h.slice(0, i).trim(), value: h.slice(i + 1).trim() };
    }
    return { key: String(h.key), value: stringify(h.value), disabled: !!h.disabled };
  }

  function headerList(list, writable) {
    const enabled = () => list.filter((h) => !h.disabled);
    const same = (a, b) => String(a).toLowerCase() === String(b).toLowerCase();
    const api = {
      get(name) {
        const h = enabled().find((x) => same(x.key, name));
        return h ? h.value : undefined;
      },
      has(name, value) {
        return enabled().some((x) => same(x.key, name) && (value === undefined || x.value === value));
      },
      one(name) {
        const h = enabled().find((x) => same(x.key, name));
        return h ? { key: h.key, value: h.value } : undefined;
      },
      all() {
        return enabled().map((h) => ({ key: h.key, value: h.value }));
      },
      toObject() {
        const o = {};
        for (const h of enabled()) o[h.key] = h.value;
        return o;
      },
      count() {
        return enabled().length;
      },
      each(fn) {
        enabled().forEach((h) => fn({ key: h.key, value: h.value }));
      },
      map(fn) {
        return api.all().map(fn);
      },
      filter(fn) {
        return api.all().filter(fn);
      },
      find(fn) {
        return api.all().find(fn);
      },
      toJSON() {
        return api.all();
      },
      toString() {
        return enabled().map((h) => h.key + ": " + h.value).join("\n");
      },
    };
    const readOnly = () => {
      throw new TypeError("These headers are read-only");
    };
    api.add = writable ? (h, v) => void list.push(parseHeader(h, v)) : readOnly;
    api.upsert = writable
      ? (h, v) => {
          const n = parseHeader(h, v);
          const existing = list.find((x) => same(x.key, n.key));
          if (existing) {
            existing.value = n.value;
            existing.disabled = false;
          } else list.push(n);
        }
      : readOnly;
    api.remove = writable
      ? (name) => {
          for (let i = list.length - 1; i >= 0; i--) {
            const hit = typeof name === "function" ? name(list[i]) : same(list[i].key, name);
            if (hit) list.splice(i, 1);
          }
        }
      : readOnly;
    api.clear = writable ? () => void list.splice(0, list.length) : readOnly;
    return api;
  }

  // -------------------------------------------------------------------------
  // URL (pm.request.url)
  // -------------------------------------------------------------------------

  function splitUrl(raw) {
    let rest = String(raw);
    let hash = "";
    const h = rest.indexOf("#");
    if (h !== -1) {
      hash = rest.slice(h);
      rest = rest.slice(0, h);
    }
    let query = [];
    const q = rest.indexOf("?");
    if (q !== -1) {
      query = rest
        .slice(q + 1)
        .split("&")
        .filter((p) => p.length)
        .map((p) => {
          const i = p.indexOf("=");
          return i === -1 ? { key: p, value: null } : { key: p.slice(0, i), value: p.slice(i + 1) };
        });
      rest = rest.slice(0, q);
    }
    return { base: rest, query, hash };
  }

  function joinUrl(parts) {
    const qs = parts.query.map((p) => (p.value === null ? p.key : p.key + "=" + p.value)).join("&");
    return parts.base + (qs ? "?" + qs : "") + parts.hash;
  }

  function makeUrl(get, set) {
    const parts = () => splitUrl(get());
    const hostAndPath = () => {
      const base = parts().base;
      const noProto = base.replace(/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//, "");
      const slash = noProto.indexOf("/");
      return slash === -1 ? { host: noProto, path: "/" } : { host: noProto.slice(0, slash), path: noProto.slice(slash) };
    };
    const editQuery = (fn) => {
      const p = parts();
      fn(p.query);
      set(joinUrl(p));
    };
    const query = {
      get(key) {
        const p = parts().query.find((x) => x.key === key);
        return p ? p.value : undefined;
      },
      has(key) {
        return parts().query.some((x) => x.key === key);
      },
      all() {
        return parts().query.map((x) => ({ key: x.key, value: x.value }));
      },
      toObject() {
        const o = {};
        for (const x of parts().query) o[x.key] = x.value;
        return o;
      },
      add(p) {
        const n = typeof p === "string" ? splitUrl("?" + p).query[0] : { key: String(p.key), value: p.value == null ? null : stringify(p.value) };
        if (n) editQuery((q) => q.push(n));
      },
      upsert(p) {
        const n = typeof p === "string" ? splitUrl("?" + p).query[0] : { key: String(p.key), value: p.value == null ? null : stringify(p.value) };
        if (!n) return;
        editQuery((q) => {
          const e = q.find((x) => x.key === n.key);
          if (e) e.value = n.value;
          else q.push(n);
        });
      },
      remove(key) {
        editQuery((q) => {
          for (let i = q.length - 1; i >= 0; i--) if (q[i].key === key) q.splice(i, 1);
        });
      },
      count() {
        return parts().query.length;
      },
      toString() {
        return parts()
          .query.map((p) => (p.value === null ? p.key : p.key + "=" + p.value))
          .join("&");
      },
    };
    return {
      toString: () => get(),
      toJSON: () => get(),
      getRaw: () => get(),
      getHost: () => hostAndPath().host,
      getPath: () => hostAndPath().path,
      getQueryString: () => query.toString(),
      getPathWithQuery: () => hostAndPath().path + (query.count() ? "?" + query.toString() : ""),
      update: (s) => set(String(s)),
      get host() {
        return hostAndPath().host.split(".");
      },
      get path() {
        return hostAndPath().path.split("/").filter((s) => s.length);
      },
      get protocol() {
        const m = /^([a-zA-Z][a-zA-Z0-9+.-]*):\/\//.exec(get());
        return m ? m[1] : undefined;
      },
      query,
    };
  }

  // -------------------------------------------------------------------------
  // pm.request
  // -------------------------------------------------------------------------

  const req = input.request; // {name, method, url, headers: [...], body: {mode, raw}}
  const request = {
    get method() {
      return req.method;
    },
    set method(m) {
      req.method = String(m).toUpperCase();
    },
    get name() {
      return req.name;
    },
    headers: headerList(req.headers, input.phase === "prerequest"),
    body: {
      get mode() {
        return req.body.mode;
      },
      get raw() {
        return req.body.raw;
      },
      set raw(v) {
        req.body.raw = stringify(v);
      },
      update(v) {
        req.body.raw = typeof v === "string" ? v : v && typeof v === "object" && "raw" in v ? stringify(v.raw) : stringify(v);
      },
      isEmpty() {
        return req.body.mode === "none" || !req.body.raw;
      },
      toString() {
        return req.body.raw;
      },
      toJSON() {
        return { mode: req.body.mode, raw: req.body.raw };
      },
    },
    addHeader(h, v) {
      request.headers.add(h, v);
    },
    upsertHeader(h, v) {
      request.headers.upsert(h, v);
    },
    removeHeader(name) {
      request.headers.remove(name);
    },
    toJSON() {
      return { method: req.method, url: req.url, header: req.headers, body: request.body.toJSON() };
    },
  };
  const url = makeUrl(
    () => req.url,
    (s) => {
      req.url = s;
    },
  );
  Object.defineProperty(request, "url", {
    enumerable: true,
    get: () => url,
    set: (v) => {
      req.url = String(v);
    },
  });

  // -------------------------------------------------------------------------
  // Responses (pm.response and pm.sendRequest results)
  // -------------------------------------------------------------------------

  function makeResponse(r) {
    // r: {code, status, headers: [{key, value}], body, responseTime, responseSize, cookies: [...]}
    const headers = r.headers.map((h) => ({ key: h.key, value: h.value }));
    const cookies = r.cookies || [];
    const res = {
      __isResponse: true,
      code: r.code,
      status: r.status,
      responseTime: r.responseTime,
      responseSize: r.responseSize,
      headers: headerList(headers, false),
      cookies: {
        get: (name) => {
          const c = cookies.find((x) => x.name === name);
          return c ? c.value : undefined;
        },
        has: (name) => cookies.some((x) => x.name === name),
        all: () => cookies.slice(),
        toObject: () => {
          const o = {};
          for (const c of cookies) o[c.name] = c.value;
          return o;
        },
      },
      text: () => r.body,
      json() {
        try {
          return JSON.parse(r.body);
        } catch (e) {
          const err = new SyntaxError("Response body is not valid JSON: " + e.message);
          err.name = "JSONError";
          throw err;
        }
      },
      reason: () => r.status,
      toJSON: () => ({ code: r.code, status: r.status, header: headers, body: r.body, responseTime: r.responseTime }),
    };
    Object.defineProperty(res, "to", { get: () => new Assertion(res).to });
    return res;
  }

  const response = input.response ? makeResponse(input.response) : undefined;

  // -------------------------------------------------------------------------
  // Assertions (a chai-style subset)
  // -------------------------------------------------------------------------

  class AssertionError extends Error {
    constructor(message) {
      super(message);
      this.name = "AssertionError";
    }
  }

  function deepEqual(a, b) {
    if (a === b) return true;
    if (typeof a === "number" && typeof b === "number") return Number.isNaN(a) && Number.isNaN(b);
    if (a instanceof Date && b instanceof Date) return a.getTime() === b.getTime();
    if (!a || !b || typeof a !== "object" || typeof b !== "object") return false;
    if (Array.isArray(a) !== Array.isArray(b)) return false;
    const ka = Object.keys(a);
    const kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    return ka.every((k) => hasOwn(b, k) && deepEqual(a[k], b[k]));
  }

  function typeOf(v) {
    if (v === null) return "null";
    if (Array.isArray(v)) return "array";
    if (v instanceof RegExp) return "regexp";
    if (v instanceof Date) return "date";
    if (v instanceof Error) return "error";
    return typeof v;
  }

  // `a.b[0].c` -> value (undefined when missing)
  function nestedGet(obj, path) {
    const parts = String(path)
      .replace(/\[(\w+)\]/g, ".$1")
      .split(".")
      .filter((s) => s.length);
    let cur = obj;
    for (const p of parts) {
      if (cur == null || !(p in Object(cur))) return { found: false };
      cur = cur[p];
    }
    return { found: true, value: cur };
  }

  function sizeOf(v) {
    if (v == null) throw new AssertionError("expected " + inspect(v) + " to have a length");
    if (typeof v === "string" || Array.isArray(v)) return v.length;
    if (v instanceof Map || v instanceof Set) return v.size;
    if (typeof v === "object") return Object.keys(v).length;
    throw new AssertionError("expected " + inspect(v) + " to have a length");
  }

  function Assertion(obj, message, flags) {
    this._obj = obj;
    this._msg = message;
    this._flags = flags || {};
  }
  const A = Assertion.prototype;

  A._assert = function (ok, msg, negMsg) {
    const pass = this._flags.negate ? !ok : !!ok;
    if (!pass) {
      const text = this._flags.negate ? negMsg : msg;
      throw new AssertionError((this._msg ? this._msg + ": " : "") + text);
    }
    return this;
  };

  const getter = (name, fn) => Object.defineProperty(A, name, { get: fn, configurable: true });
  for (const w of ["to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same", "but", "does", "still", "also"]) {
    getter(w, function () {
      return this;
    });
  }
  getter("not", function () {
    this._flags.negate = !this._flags.negate;
    return this;
  });
  for (const f of ["deep", "own", "nested", "any", "all", "ordered"]) {
    getter(f, function () {
      this._flags[f] = true;
      return this;
    });
  }

  // A property that is also callable: `to.include('x')` and `to.include.keys('x')`,
  // `to.have.length(3)` and `to.have.length.above(2)`.
  function chainable(names, flag, call) {
    for (const name of names) {
      getter(name, function () {
        const self = this;
        self._flags[flag] = true;
        const f = function (...args) {
          self._flags[flag] = false;
          return call.apply(self, args);
        };
        Object.setPrototypeOf(f, self);
        return f;
      });
    }
  }

  chainable(["include", "includes", "contain", "contains"], "contains", function (val) {
    const o = this._obj;
    const eq = (a, b) => (this._flags.deep ? deepEqual(a, b) : a === b);
    let ok;
    if (typeof o === "string") ok = o.indexOf(String(val)) !== -1;
    else if (Array.isArray(o)) ok = o.some((x) => eq(x, val) || (!this._flags.deep && deepEqual(x, val) && typeof val === "object"));
    else if (o && typeof o === "object") ok = val && typeof val === "object" && Object.keys(val).every((k) => hasOwn(o, k) && deepEqual(o[k], val[k]));
    else throw new AssertionError("expected " + inspect(o) + " to be a string, array or object");
    return this._assert(ok, "expected " + inspect(o) + " to include " + inspect(val), "expected " + inspect(o) + " to not include " + inspect(val));
  });

  chainable(["length", "lengthOf"], "length", function (n) {
    const len = sizeOf(this._obj);
    return this._assert(len === n, "expected " + inspect(this._obj) + " to have a length of " + n + " but got " + len, "expected " + inspect(this._obj) + " to not have a length of " + n);
  });

  // Subject of numeric comparisons: the length after `.length`, else the value.
  A._num = function () {
    return this._flags.length ? sizeOf(this._obj) : this._obj;
  };
  const what = (self) => (self._flags.length ? "length of " + inspect(self._obj) : inspect(self._obj));

  function method(names, fn) {
    for (const n of names) A[n] = fn;
  }

  method(["equal", "equals", "eq"], function (v) {
    if (this._flags.deep) return this.eql(v);
    return this._assert(this._obj === v, "expected " + inspect(this._obj) + " to equal " + inspect(v), "expected " + inspect(this._obj) + " to not equal " + inspect(v));
  });
  method(["eql", "eqls"], function (v) {
    return this._assert(deepEqual(this._obj, v), "expected " + inspect(this._obj) + " to deeply equal " + inspect(v), "expected " + inspect(this._obj) + " to not deeply equal " + inspect(v));
  });
  method(["above", "gt", "greaterThan"], function (n) {
    return this._assert(this._num() > n, "expected " + what(this) + " to be above " + n, "expected " + what(this) + " to be at most " + n);
  });
  method(["least", "gte"], function (n) {
    return this._assert(this._num() >= n, "expected " + what(this) + " to be at least " + n, "expected " + what(this) + " to be below " + n);
  });
  method(["below", "lt", "lessThan"], function (n) {
    return this._assert(this._num() < n, "expected " + what(this) + " to be below " + n, "expected " + what(this) + " to be at least " + n);
  });
  method(["most", "lte"], function (n) {
    return this._assert(this._num() <= n, "expected " + what(this) + " to be at most " + n, "expected " + what(this) + " to be above " + n);
  });
  method(["within"], function (lo, hi) {
    const v = this._num();
    return this._assert(v >= lo && v <= hi, "expected " + what(this) + " to be within " + lo + ".." + hi, "expected " + what(this) + " to not be within " + lo + ".." + hi);
  });
  method(["closeTo", "approximately"], function (expected, delta) {
    return this._assert(Math.abs(this._obj - expected) <= delta, "expected " + inspect(this._obj) + " to be close to " + expected + " +/- " + delta, "expected " + inspect(this._obj) + " not to be close to " + expected + " +/- " + delta);
  });
  method(["a", "an"], function (type) {
    const t = String(type).toLowerCase();
    const actual = typeOf(this._obj);
    const article = /^[aeiou]/.test(t) ? "an " : "a ";
    return this._assert(actual === t, "expected " + inspect(this._obj) + " to be " + article + t + " but got " + actual, "expected " + inspect(this._obj) + " not to be " + article + t);
  });
  method(["instanceOf", "instanceof"], function (ctor) {
    return this._assert(this._obj instanceof ctor, "expected " + inspect(this._obj) + " to be an instance of " + (ctor && ctor.name), "expected " + inspect(this._obj) + " to not be an instance of " + (ctor && ctor.name));
  });
  method(["property"], function (name, ...rest) {
    const o = this._obj;
    let found, value;
    if (this._flags.nested) ({ found, value } = nestedGet(o, name));
    else {
      found = o != null && (this._flags.own ? hasOwn(o, name) : name in Object(o));
      value = found ? o[name] : undefined;
    }
    const label = (this._flags.nested ? "nested " : this._flags.own ? "own " : "") + "property " + inspect(name);
    if (rest.length) {
      const v = rest[0];
      const ok = found && (this._flags.deep ? deepEqual(value, v) : value === v || (typeof v === "object" && deepEqual(value, v)));
      this._assert(ok, "expected " + inspect(o) + " to have " + label + " of " + inspect(v) + (found ? ", but got " + inspect(value) : ""), "expected " + inspect(o) + " to not have " + label + " of " + inspect(v));
    } else {
      this._assert(found, "expected " + inspect(o) + " to have " + label, "expected " + inspect(o) + " to not have " + label);
    }
    if (!this._flags.negate) this._obj = value;
    return this;
  });
  method(["ownProperty", "haveOwnProperty"], function (name, ...rest) {
    this._flags.own = true;
    return this.property(name, ...rest);
  });
  method(["nestedProperty"], function (name, ...rest) {
    this._flags.nested = true;
    return this.property(name, ...rest);
  });
  method(["keys", "key"], function (...args) {
    const want = args.length === 1 && Array.isArray(args[0]) ? args[0] : args.length === 1 && args[0] && typeof args[0] === "object" ? Object.keys(args[0]) : args;
    const have = this._obj && typeof this._obj === "object" ? Object.keys(this._obj) : [];
    let ok;
    if (this._flags.any) ok = want.some((k) => have.includes(String(k)));
    else if (this._flags.contains) ok = want.every((k) => have.includes(String(k)));
    else ok = want.length === have.length && want.every((k) => have.includes(String(k)));
    const mode = this._flags.any ? "any of" : this._flags.contains ? "" : "all of";
    return this._assert(ok, "expected " + inspect(this._obj) + " to have " + (mode ? mode + " " : "") + "keys " + want.map(inspect).join(", "), "expected " + inspect(this._obj) + " to not have keys " + want.map(inspect).join(", "));
  });
  method(["members"], function (list) {
    const o = Array.isArray(this._obj) ? this._obj : [];
    const contains = (arr, x) => arr.some((y) => deepEqual(x, y));
    const ok = this._flags.contains ? list.every((x) => contains(o, x)) : list.length === o.length && list.every((x) => contains(o, x)) && o.every((x) => contains(list, x));
    return this._assert(ok, "expected " + inspect(o) + " to have the same members as " + inspect(list), "expected " + inspect(o) + " to not have the same members as " + inspect(list));
  });
  method(["oneOf"], function (list) {
    const ok = list.some((x) => (this._flags.deep ? deepEqual(x, this._obj) : x === this._obj));
    return this._assert(ok, "expected " + inspect(this._obj) + " to be one of " + inspect(list), "expected " + inspect(this._obj) + " to not be one of " + inspect(list));
  });
  method(["match", "matches"], function (re) {
    return this._assert(re.test(String(this._obj)), "expected " + inspect(this._obj) + " to match " + re, "expected " + inspect(this._obj) + " not to match " + re);
  });
  method(["string"], function (s) {
    return this._assert(String(this._obj).indexOf(s) !== -1, "expected " + inspect(this._obj) + " to contain " + inspect(s), "expected " + inspect(this._obj) + " to not contain " + inspect(s));
  });
  method(["satisfy", "satisfies"], function (fn) {
    return this._assert(fn(this._obj), "expected " + inspect(this._obj) + " to satisfy " + inspect(fn), "expected " + inspect(this._obj) + " to not satisfy " + inspect(fn));
  });
  method(["throw", "throws", "Throw"], function (expected) {
    let thrown = null;
    try {
      this._obj();
    } catch (e) {
      thrown = e;
    }
    let ok = thrown !== null;
    if (ok && expected !== undefined) {
      const m = thrown && thrown.message !== undefined ? thrown.message : String(thrown);
      if (typeof expected === "string") ok = m.indexOf(expected) !== -1;
      else if (expected instanceof RegExp) ok = expected.test(m);
      else if (typeof expected === "function") ok = thrown instanceof expected;
    }
    return this._assert(ok, "expected function to throw" + (expected !== undefined ? " " + inspect(expected) : ""), "expected function to not throw, but it threw " + inspect(thrown));
  });
  method(["jsonSchema"], function (schema) {
    const subject = this._obj && this._obj.__isResponse ? this._obj.json() : this._obj;
    const errors = validateSchema(subject, schema, "data");
    return this._assert(errors.length === 0, "expected data to satisfy schema but found error: " + (errors[0] || ""), "expected data to not satisfy schema");
  });

  // Property-style assertions; on a response some of them check the status (Postman).
  const isRes = (o) => o && o.__isResponse;
  const statusGetter = (name, test, label) => {
    getter(name, function () {
      if (!isRes(this._obj)) throw new AssertionError("expected " + inspect(this._obj) + " to be a response");
      return this._assert(test(this._obj.code), "expected response to " + label + " but got " + this._obj.code, "expected response to not " + label);
    });
  };
  getter("ok", function () {
    if (isRes(this._obj)) return this._assert(this._obj.code === 200, "expected response code to be 200 but found " + this._obj.code, "expected response code to not be 200");
    return this._assert(!!this._obj, "expected " + inspect(this._obj) + " to be truthy", "expected " + inspect(this._obj) + " to be falsy");
  });
  statusGetter("success", (c) => c >= 200 && c < 300, "be a success (2xx)");
  statusGetter("info", (c) => c >= 100 && c < 200, "be informational (1xx)");
  statusGetter("redirection", (c) => c >= 300 && c < 400, "be a redirection (3xx)");
  statusGetter("clientError", (c) => c >= 400 && c < 500, "be a client error (4xx)");
  statusGetter("serverError", (c) => c >= 500, "be a server error (5xx)");
  statusGetter("error", (c) => c >= 400, "be an error (4xx/5xx)");
  statusGetter("accepted", (c) => c === 202, "be 202 Accepted");
  statusGetter("badRequest", (c) => c === 400, "be 400 Bad Request");
  statusGetter("unauthorized", (c) => c === 401, "be 401 Unauthorized");
  statusGetter("forbidden", (c) => c === 403, "be 403 Forbidden");
  statusGetter("notFound", (c) => c === 404, "be 404 Not Found");
  statusGetter("rateLimited", (c) => c === 429, "be 429 Too Many Requests");
  getter("json", function () {
    let ok = true;
    try {
      this._obj.json();
    } catch (e) {
      ok = false;
    }
    return this._assert(ok, "expected response body to be valid JSON", "expected response body to not be valid JSON");
  });
  getter("withBody", function () {
    return this._assert(isRes(this._obj) && this._obj.text().length > 0, "expected response to have a body", "expected response to not have a body");
  });
  getter("true", function () {
    return this._assert(this._obj === true, "expected " + inspect(this._obj) + " to be true", "expected " + inspect(this._obj) + " to not be true");
  });
  getter("false", function () {
    return this._assert(this._obj === false, "expected " + inspect(this._obj) + " to be false", "expected " + inspect(this._obj) + " to not be false");
  });
  getter("null", function () {
    return this._assert(this._obj === null, "expected " + inspect(this._obj) + " to be null", "expected " + inspect(this._obj) + " to not be null");
  });
  getter("undefined", function () {
    return this._assert(this._obj === undefined, "expected " + inspect(this._obj) + " to be undefined", "expected " + inspect(this._obj) + " to not be undefined");
  });
  getter("NaN", function () {
    return this._assert(Number.isNaN(this._obj), "expected " + inspect(this._obj) + " to be NaN", "expected " + inspect(this._obj) + " to not be NaN");
  });
  getter("exist", function () {
    return this._assert(this._obj != null, "expected " + inspect(this._obj) + " to exist", "expected " + inspect(this._obj) + " to not exist");
  });
  getter("empty", function () {
    const o = this._obj;
    const len = typeof o === "string" || Array.isArray(o) ? o.length : o && typeof o === "object" ? Object.keys(o).length : -1;
    if (len === -1) throw new AssertionError(".empty was passed non-string primitive " + inspect(o));
    return this._assert(len === 0, "expected " + inspect(o) + " to be empty", "expected " + inspect(o) + " not to be empty");
  });

  // Response-only methods.
  A.status = function (s) {
    const r = this._obj;
    if (!isRes(r)) throw new AssertionError("expected " + inspect(r) + " to be a response");
    if (typeof s === "number") return this._assert(r.code === s, "expected response to have status code " + s + " but got " + r.code, "expected response to not have status code " + s);
    return this._assert(String(r.status).toLowerCase() === String(s).toLowerCase(), "expected response to have status reason " + inspect(s) + " but got " + inspect(r.status), "expected response to not have status reason " + inspect(s));
  };
  A.header = function (name, value) {
    const r = this._obj;
    if (!isRes(r)) throw new AssertionError("expected " + inspect(r) + " to be a response");
    const v = r.headers.get(name);
    if (value === undefined) return this._assert(v !== undefined, "expected response to have header " + inspect(name), "expected response to not have header " + inspect(name));
    return this._assert(v === value, "expected response to have header " + inspect(name) + " with value " + inspect(value) + (v === undefined ? "" : " but got " + inspect(v)), "expected response to not have header " + inspect(name) + " with value " + inspect(value));
  };
  A.body = function (expected) {
    const r = this._obj;
    if (!isRes(r)) throw new AssertionError("expected " + inspect(r) + " to be a response");
    if (expected === undefined) return this._assert(r.text().length > 0, "expected response to have content in body", "expected response to not have content in body");
    if (typeof expected === "string") return this._assert(r.text() === expected, "expected response body to equal " + inspect(expected), "expected response body to not equal " + inspect(expected));
    if (expected instanceof RegExp) return this._assert(expected.test(r.text()), "expected response body to match " + expected, "expected response body to not match " + expected);
    let json;
    try {
      json = r.json();
    } catch (e) {
      json = undefined;
    }
    return this._assert(deepEqual(json, expected), "expected response body json to equal " + inspect(expected) + " but got " + inspect(json), "expected response body json to not equal " + inspect(expected));
  };
  A.jsonBody = function (path, ...rest) {
    const r = this._obj;
    if (!isRes(r)) throw new AssertionError("expected " + inspect(r) + " to be a response");
    let json;
    try {
      json = r.json();
    } catch (e) {
      return this._assert(false, "expected response body to be a valid json", "expected response body not to be a valid json");
    }
    if (path === undefined) return this._assert(true, "", "expected response body not to be a valid json");
    if (typeof path === "object") return this._assert(deepEqual(json, path), "expected response body json to equal " + inspect(path) + " but got " + inspect(json), "expected response body json to not equal " + inspect(path));
    const got = nestedGet(json, path);
    if (!rest.length) return this._assert(got.found, "expected " + inspect(json) + " to have property " + inspect(path), "expected " + inspect(json) + " to not have property " + inspect(path));
    return this._assert(got.found && deepEqual(got.value, rest[0]), "expected response body json at " + inspect(path) + " to contain " + inspect(rest[0]) + " but got " + inspect(got.value), "expected response body json at " + inspect(path) + " to not contain " + inspect(rest[0]));
  };
  A.responseTime = function () {
    // `pm.response.to.have.responseTime.below(200)` style is rare; support the value form.
    this._obj = this._obj.responseTime;
    return this;
  };

  // Minimal JSON Schema (draft 4-7 subset) used by `.jsonSchema(schema)`.
  function validateSchema(v, s, path) {
    const errors = [];
    if (!s || typeof s !== "object") return errors;
    const err = (m) => errors.push(path + " " + m);
    const types = s.type === undefined ? null : Array.isArray(s.type) ? s.type : [s.type];
    if (types) {
      const t = typeOf(v);
      const ok = types.some((x) => x === t || (x === "integer" && Number.isInteger(v)) || (x === "number" && t === "number"));
      if (!ok) {
        err("should be " + types.join(" or ") + " but is " + t);
        return errors;
      }
    }
    if (s.enum && !s.enum.some((x) => deepEqual(x, v))) err("should be one of " + inspect(s.enum));
    if ("const" in s && !deepEqual(s.const, v)) err("should be equal to " + inspect(s.const));
    if (typeof v === "number") {
      if (s.minimum !== undefined && v < s.minimum) err("should be >= " + s.minimum);
      if (s.maximum !== undefined && v > s.maximum) err("should be <= " + s.maximum);
      if (typeof s.exclusiveMinimum === "number" && v <= s.exclusiveMinimum) err("should be > " + s.exclusiveMinimum);
      if (typeof s.exclusiveMaximum === "number" && v >= s.exclusiveMaximum) err("should be < " + s.exclusiveMaximum);
    }
    if (typeof v === "string") {
      if (s.minLength !== undefined && v.length < s.minLength) err("should NOT be shorter than " + s.minLength + " characters");
      if (s.maxLength !== undefined && v.length > s.maxLength) err("should NOT be longer than " + s.maxLength + " characters");
      if (s.pattern && !new RegExp(s.pattern).test(v)) err('should match pattern "' + s.pattern + '"');
    }
    if (Array.isArray(v)) {
      if (s.minItems !== undefined && v.length < s.minItems) err("should NOT have fewer than " + s.minItems + " items");
      if (s.maxItems !== undefined && v.length > s.maxItems) err("should NOT have more than " + s.maxItems + " items");
      if (s.items && !Array.isArray(s.items)) v.forEach((x, i) => errors.push(...validateSchema(x, s.items, path + "[" + i + "]")));
      if (s.uniqueItems && v.some((x, i) => v.findIndex((y) => deepEqual(x, y)) !== i)) err("should NOT have duplicate items");
    }
    if (v && typeof v === "object" && !Array.isArray(v)) {
      for (const k of s.required || []) if (!hasOwn(v, k)) err("should have required property '" + k + "'");
      const props = s.properties || {};
      for (const k of Object.keys(props)) if (hasOwn(v, k)) errors.push(...validateSchema(v[k], props[k], path + "." + k));
      if (s.additionalProperties === false) {
        for (const k of Object.keys(v)) if (!hasOwn(props, k)) err("should NOT have additional property '" + k + "'");
      } else if (s.additionalProperties && typeof s.additionalProperties === "object") {
        for (const k of Object.keys(v)) if (!hasOwn(props, k)) errors.push(...validateSchema(v[k], s.additionalProperties, path + "." + k));
      }
    }
    if (s.allOf) for (const sub of s.allOf) errors.push(...validateSchema(v, sub, path));
    if (s.anyOf && !s.anyOf.some((sub) => validateSchema(v, sub, path).length === 0)) err("should match some schema in anyOf");
    if (s.oneOf && s.oneOf.filter((sub) => validateSchema(v, sub, path).length === 0).length !== 1) err("should match exactly one schema in oneOf");
    if (s.not && validateSchema(v, s.not, path).length === 0) err("should NOT be valid");
    return errors;
  }

  // -------------------------------------------------------------------------
  // pm.test
  // -------------------------------------------------------------------------

  function test(name, fn) {
    const r = { name: String(name), passed: true, skipped: false, error: null };
    out.tests.push(r);
    try {
      const ret = typeof fn === "function" ? fn() : undefined;
      if (ret && typeof ret.then === "function") {
        ret.then(null, (e) => {
          r.passed = false;
          r.error = errMsg(e);
        });
      }
    } catch (e) {
      r.passed = false;
      r.error = errMsg(e);
    }
  }
  test.skip = function (name) {
    out.tests.push({ name: String(name), passed: true, skipped: true, error: null });
  };

  // -------------------------------------------------------------------------
  // pm.sendRequest
  // -------------------------------------------------------------------------

  function normHeaders(h) {
    if (!h) return [];
    if (typeof h === "string") return h.split("\n").filter((l) => l.trim()).map((l) => parseHeader(l));
    if (Array.isArray(h)) return h.filter((x) => !x.disabled).map((x) => parseHeader(x));
    return Object.keys(h).map((k) => ({ key: k, value: stringify(h[k]) }));
  }

  function normBody(b) {
    if (b === undefined || b === null) return null;
    if (typeof b === "string") return { raw: b };
    const pairs = (list) => (list || []).filter((x) => !x.disabled).map((x) => [String(x.key), stringify(x.value)]);
    switch (b.mode) {
      case "urlencoded":
        return { urlencoded: pairs(b.urlencoded) };
      case "formdata":
        return { formdata: pairs(b.formdata) };
      case "graphql":
        return { raw: JSON.stringify({ query: (b.graphql || {}).query || "", variables: (b.graphql || {}).variables || {} }), contentType: "application/json" };
      default:
        return { raw: stringify(b.raw) };
    }
  }

  function sendRequest(spec, callback) {
    const s = typeof spec === "string" ? { url: spec } : spec || {};
    const payload = {
      url: String(s.url),
      method: String(s.method || "GET").toUpperCase(),
      headers: normHeaders(s.header || s.headers),
      body: normBody(s.body),
    };
    const res = JSON.parse(globalThis.__send(JSON.stringify(payload)));
    if (res.index !== undefined && res.index !== null) out.events.push({ kind: "http", index: res.index });
    const err = res.error ? new Error(res.error) : null;
    const response = res.response ? makeResponse(res.response) : null;
    if (typeof callback === "function") {
      callback(err, response);
      return undefined;
    }
    return err ? Promise.reject(err) : Promise.resolve(response);
  }

  // -------------------------------------------------------------------------
  // pm, legacy globals, misc
  // -------------------------------------------------------------------------

  const execution = {
    setNextRequest(name) {
      out.nextRequest = { name: name === null || name === undefined ? null : String(name) };
    },
    skipRequest() {
      if (input.phase !== "prerequest") throw new Error("pm.execution.skipRequest() can only be used in a pre-request script");
      out.skip = true;
    },
    get location() {
      return [input.info.requestName];
    },
  };

  const pm = {
    environment,
    collectionVariables: makeScope("collection", "pm.collectionVariables", true),
    globals: makeScope("globals", "pm.globals", true),
    iterationData: makeScope("data", "pm.iterationData", false),
    variables,
    request,
    response,
    cookies: response ? response.cookies : { get: () => undefined, has: () => false, all: () => [], toObject: () => ({}) },
    info: {
      eventName: input.phase,
      iteration: input.info.iteration,
      iterationCount: input.info.iterationCount,
      requestName: input.info.requestName,
      requestId: input.info.requestId,
    },
    test,
    expect: (v, message) => new Assertion(v, message),
    sendRequest,
    execution,
    require: (name) => globalThis.require(name),
  };
  globalThis.pm = pm;

  globalThis.tests = {};
  globalThis.postman = {
    setNextRequest: execution.setNextRequest,
    setEnvironmentVariable: (k, v) => environment.set(k, v),
    getEnvironmentVariable: (k) => environment.get(k),
    clearEnvironmentVariable: (k) => environment.unset(k),
    clearEnvironmentVariables: () => environment.clear(),
    setGlobalVariable: (k, v) => pm.globals.set(k, v),
    getGlobalVariable: (k) => pm.globals.get(k),
    clearGlobalVariable: (k) => pm.globals.unset(k),
    clearGlobalVariables: () => pm.globals.clear(),
    getResponseHeader: (k) => (response ? response.headers.get(k) : undefined),
  };
  globalThis.environment = environment.toObject();
  globalThis.globals = pm.globals.toObject();
  globalThis.data = pm.iterationData.toObject();
  globalThis.iteration = input.info.iteration;
  if (response) {
    globalThis.responseBody = response.text();
    globalThis.responseCode = { code: response.code, name: response.status, detail: response.status };
    globalThis.responseHeaders = response.headers.toObject();
    globalThis.responseTime = response.responseTime;
  }
  globalThis.request = { url: req.url, method: req.method, headers: request.headers.toObject(), data: req.body.raw, name: req.name };

  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  globalThis.btoa = function (input) {
    const s = String(input);
    let outStr = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i);
      const b = s.charCodeAt(i + 1);
      const c = s.charCodeAt(i + 2);
      if (a > 255 || b > 255 || c > 255) throw new Error("btoa: string contains characters outside of the Latin1 range");
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      outStr += B64[(n >> 18) & 63] + B64[(n >> 12) & 63] + (i + 1 < s.length ? B64[(n >> 6) & 63] : "=") + (i + 2 < s.length ? B64[n & 63] : "=");
    }
    return outStr;
  };
  globalThis.atob = function (input) {
    const s = String(input).replace(/[\s=]/g, "");
    let outStr = "";
    let bits = 0;
    let acc = 0;
    for (const ch of s) {
      const v = B64.indexOf(ch);
      if (v === -1) throw new Error("atob: invalid base64 character");
      acc = (acc << 6) | v;
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        outStr += String.fromCharCode((acc >> bits) & 255);
      }
    }
    return outStr;
  };

  // -------------------------------------------------------------------------
  // Entry points used by Rust
  // -------------------------------------------------------------------------

  // The user script is wrapped in an async function so `await pm.sendRequest(...)` works.
  globalThis.__run = function (fn) {
    try {
      const p = fn();
      if (p && typeof p.then === "function") {
        p.then(null, (e) => {
          out.error = errMsg(e);
        });
      }
    } catch (e) {
      out.error = errMsg(e);
    }
  };

  globalThis.__finish = function () {
    for (const k of Object.keys(globalThis.tests || {})) {
      const ok = !!globalThis.tests[k];
      out.tests.push({ name: k, passed: ok, skipped: false, error: ok ? null : "Test failed" });
    }
    if (input.phase === "prerequest") out.request = req;
    return JSON.stringify(out);
  };
})();
