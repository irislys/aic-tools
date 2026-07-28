use crate::mem::Process;

const MONO_MOD: &str = "mono-2.0-bdwgc.dll";

pub fn find_default_cane(proc: &Process) -> Result<u64, String> {
    let api = MonoApi::resolve(proc)?;
    let s = RemoteStrings::alloc(
        proc,
        &["Assembly-CSharp", "nel", "CaneManager", "DefaultCane"],
    )?;
    let domain = api.root_domain(proc)?;
    let _ = api.thread_attach(proc, domain);
    let image = api.image_loaded(proc, s[0])?;
    let klass = api.class_from_name(proc, image, s[1], s[2])?;
    let field = api.field_from_name(proc, klass, s[3])?;
    let field_off = api.field_offset(proc, field)?;
    if !(0..=0x10000).contains(&field_off) {
        return Err(format!("invalid field offset {field_off}"));
    }
    let vtable = api.class_vtable(proc, domain, klass)?;
    let static_data = api.vtable_static_data(proc, vtable, field)?;
    let addr = static_data.wrapping_add(field_off as u64);
    let cane = proc
        .read_u64(addr)
        .ok_or_else(|| format!("read DefaultCane at {addr:#x} failed"))?;
    if cane == 0 || !proc.looks_like_user_ptr(cane) {
        return Err(format!("DefaultCane invalid: {cane:#x}"));
    }
    Ok(cane)
}

pub fn find_method(
    proc: &Process,
    image_name: &str,
    ns: &str,
    class_name: &str,
    method_name: &str,
    param_count: i32,
) -> Result<u64, String> {
    let api = MonoApi::resolve(proc)?;
    let s = RemoteStrings::alloc(proc, &[image_name, ns, class_name, method_name])?;
    let domain = api.root_domain(proc)?;
    let _ = api.thread_attach(proc, domain);
    let image = api.image_loaded(proc, s[0])?;
    let klass = api.class_from_name(proc, image, s[1], s[2])?;
    api.method_from_name(proc, klass, s[3], param_count)
}

pub fn compile_method(proc: &Process, method: u64) -> Result<u64, String> {
    if method == 0 {
        return Err("null method".into());
    }
    if proc.is_alive() == Some(false) {
        return Err("游戏进程已退出".into());
    }
    let api = MonoApi::resolve(proc)?;
    let addr = proc.remote_mono_compile(
        api.get_root_domain,
        api.thread_attach,
        api.domain_set,
        api.compile_method,
        method,
        60_000,
    )?;
    if proc.is_alive() == Some(false) {
        return Err("mono_compile_method 导致游戏进程退出".into());
    }
    if addr == 0 {
        return Err("mono_compile_method returned null".into());
    }
    Ok(addr)
}

struct MonoApi {
    get_root_domain: u64,
    thread_attach: u64,
    domain_set: u64,
    image_loaded: u64,
    class_from_name: u64,
    class_get_field_from_name: u64,
    field_get_offset: u64,
    class_vtable: u64,
    vtable_get_static_field_data: u64,
    class_get_method_from_name: u64,
    compile_method: u64,
}

impl MonoApi {
    fn resolve(proc: &Process) -> Result<Self, String> {
        let module = proc.module_by_name(MONO_MOD).ok_or("mono not loaded")?;
        const NAMES: &[&str] = &[
            "mono_get_root_domain",
            "mono_thread_attach",
            "mono_image_loaded",
            "mono_class_from_name",
            "mono_class_get_field_from_name",
            "mono_field_get_offset",
            "mono_class_vtable",
            "mono_vtable_get_static_field_data",
            "mono_class_get_method_from_name",
            "mono_compile_method",
        ];
        let f = resolve_exports(proc, module.base, NAMES)?;
        let domain_set = resolve_export_one(proc, module.base, "mono_domain_set").unwrap_or(0);
        Ok(Self {
            get_root_domain: f[0],
            thread_attach: f[1],
            domain_set,
            image_loaded: f[2],
            class_from_name: f[3],
            class_get_field_from_name: f[4],
            field_get_offset: f[5],
            class_vtable: f[6],
            vtable_get_static_field_data: f[7],
            class_get_method_from_name: f[8],
            compile_method: f[9],
        })
    }

    fn call(proc: &Process, func: u64, args: &[u64]) -> Result<u64, String> {
        proc.remote_call(func, args, 15_000)
    }

    fn root_domain(&self, proc: &Process) -> Result<u64, String> {
        let d = Self::call(proc, self.get_root_domain, &[])
            .map_err(|e| format!("mono_get_root_domain: {e}"))?;
        if d == 0 {
            return Err("mono_get_root_domain returned null".into());
        }
        Ok(d)
    }

    fn thread_attach(&self, proc: &Process, domain: u64) -> Result<u64, String> {
        Self::call(proc, self.thread_attach, &[domain])
    }

    fn image_loaded(&self, proc: &Process, name_ptr: u64) -> Result<u64, String> {
        let img = Self::call(proc, self.image_loaded, &[name_ptr])
            .map_err(|e| format!("mono_image_loaded: {e}"))?;
        if img == 0 {
            return Err("Assembly image not loaded".into());
        }
        Ok(img)
    }

    fn class_from_name(
        &self,
        proc: &Process,
        image: u64,
        ns: u64,
        name: u64,
    ) -> Result<u64, String> {
        let k = Self::call(proc, self.class_from_name, &[image, ns, name])
            .map_err(|e| format!("mono_class_from_name: {e}"))?;
        if k == 0 {
            return Err("class not found".into());
        }
        Ok(k)
    }

    fn field_from_name(&self, proc: &Process, klass: u64, name: u64) -> Result<u64, String> {
        let f = Self::call(proc, self.class_get_field_from_name, &[klass, name])
            .map_err(|e| format!("mono_class_get_field_from_name: {e}"))?;
        if f == 0 {
            return Err("field not found".into());
        }
        Ok(f)
    }

    fn field_offset(&self, proc: &Process, field: u64) -> Result<i64, String> {
        Ok(Self::call(proc, self.field_get_offset, &[field])
            .map_err(|e| format!("mono_field_get_offset: {e}"))? as i32 as i64)
    }

    fn class_vtable(&self, proc: &Process, domain: u64, klass: u64) -> Result<u64, String> {
        let v = Self::call(proc, self.class_vtable, &[domain, klass])
            .map_err(|e| format!("mono_class_vtable: {e}"))?;
        if v == 0 {
            return Err("mono_class_vtable returned null".into());
        }
        Ok(v)
    }

    fn vtable_static_data(&self, proc: &Process, vtable: u64, field: u64) -> Result<u64, String> {
        let d = Self::call(proc, self.vtable_get_static_field_data, &[vtable, field])
            .map_err(|e| format!("mono_vtable_get_static_field_data: {e}"))?;
        if d == 0 {
            return Err("static field data is null".into());
        }
        Ok(d)
    }

    fn method_from_name(
        &self,
        proc: &Process,
        klass: u64,
        name: u64,
        param_count: i32,
    ) -> Result<u64, String> {
        let m = Self::call(
            proc,
            self.class_get_method_from_name,
            &[klass, name, param_count as u64],
        )
        .map_err(|e| format!("mono_class_get_method_from_name: {e}"))?;
        if m == 0 {
            return Err("method not found".into());
        }
        Ok(m)
    }
}

fn resolve_export_one(proc: &Process, mono_base: u64, name: &str) -> Option<u64> {
    resolve_exports(proc, mono_base, &[name]).ok().map(|v| v[0])
}

fn resolve_exports(proc: &Process, mono_base: u64, names: &[&str]) -> Result<Vec<u64>, String> {
    let mut found = vec![0u64; names.len()];
    let read_u32 = |addr: u64, what: &str| -> Result<u32, String> {
        proc.read_u32(addr)
            .ok_or_else(|| format!("read {what} failed"))
    };

    let e_lfanew = read_u32(mono_base + 0x3C, "PE e_lfanew")? as u64;
    let pe = mono_base + e_lfanew;
    let magic = read_u32(pe + 0x18, "PE magic")? as u16;
    if magic != 0x20B {
        return Err("mono module is not PE32+".into());
    }
    let export_rva = read_u32(pe + 0x18 + 0x70, "export RVA")? as u64;
    let export_size = read_u32(pe + 0x18 + 0x74, "export size")? as u64;
    if export_rva == 0 || export_size == 0 {
        return Err("mono export directory empty".into());
    }
    let exp = mono_base + export_rva;
    let n_names = read_u32(exp + 0x18, "export name count")? as u64;
    let addr_funcs = mono_base + read_u32(exp + 0x1C, "AddressOfFunctions")? as u64;
    let addr_names = mono_base + read_u32(exp + 0x20, "AddressOfNames")? as u64;
    let addr_ords = mono_base + read_u32(exp + 0x24, "AddressOfNameOrdinals")? as u64;

    let mut remaining = names.len();
    for i in 0..n_names {
        if remaining == 0 {
            break;
        }
        let name_rva = read_u32(addr_names + i * 4, "name RVA")? as u64;
        let remote_name = proc
            .read_c_string(mono_base + name_rva, 256)
            .ok_or("read export name failed")?;
        let Some(idx) = names.iter().position(|&n| n == remote_name) else {
            continue;
        };
        let ord = proc
            .read_u16(addr_ords + i * 2)
            .ok_or("read ordinal failed")? as u64;
        let func_rva = read_u32(addr_funcs + ord * 4, "func RVA")? as u64;
        if !(export_rva..export_rva + export_size).contains(&func_rva) {
            found[idx] = mono_base + func_rva;
            remaining -= 1;
        }
    }
    if remaining > 0 {
        let missing: Vec<_> = names
            .iter()
            .zip(found.iter())
            .filter(|&(_, v)| *v == 0)
            .map(|(n, _)| *n)
            .collect();
        return Err(format!("missing mono exports: {}", missing.join(", ")));
    }
    Ok(found)
}

struct RemoteStrings<'a> {
    proc: &'a Process,
    addrs: Vec<u64>,
}

impl<'a> RemoteStrings<'a> {
    fn alloc(proc: &'a Process, texts: &[&str]) -> Result<Self, String> {
        let mut addrs = Vec::with_capacity(texts.len());
        for t in texts {
            match proc.alloc_remote_string(t) {
                Some(a) => addrs.push(a),
                None => {
                    for a in &addrs {
                        proc.free_remote(*a);
                    }
                    return Err(format!("alloc string {t}"));
                }
            }
        }
        Ok(Self { proc, addrs })
    }
}

impl std::ops::Index<usize> for RemoteStrings<'_> {
    type Output = u64;
    fn index(&self, i: usize) -> &u64 {
        &self.addrs[i]
    }
}

impl Drop for RemoteStrings<'_> {
    fn drop(&mut self) {
        for a in self.addrs.drain(..) {
            self.proc.free_remote(a);
        }
    }
}
