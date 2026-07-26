use crate::offsets::*;

#[derive(Clone, Copy, Debug)]
pub struct PrCaneFields {
    pub near_power: f32,
    pub near_shotgun: f32,
    pub near_reach: f32,
    pub near_punch: f32,
    pub magic_prepare: f32,
    pub drain_after_lock: f32,
    pub lockon: f32,
    pub far_power: f32,
    pub mana_splash: f32,
    pub mp_use: f32,
    pub stability: f32,
    pub neutral: f32,
    pub castspeed: f32,
    pub castspeed_overhold: f32,
}

impl Default for PrCaneFields {
    fn default() -> Self {
        Self {
            near_power: 1.0,
            near_shotgun: 1.0,
            near_reach: 1.0,
            near_punch: 1.0,
            magic_prepare: 1.0,
            drain_after_lock: 1.0,
            lockon: 1.0,
            far_power: 1.0,
            mana_splash: 1.0,
            mp_use: 1.0,
            stability: 1.0,
            neutral: 1.0,
            castspeed: 1.0,
            castspeed_overhold: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AxisDef {
    pub id: u8,
    pub name_zh: &'static str,
    pub formula_hint: &'static str,
}

pub const AXES: &[AxisDef] = &[
    AxisDef {
        id: 0,
        name_zh: "近战攻击距离",
        formula_hint: "显示值 ≈ 50 × near_reach  →  字段 = 显示值 / 50",
    },
    AxisDef {
        id: 1,
        name_zh: "近战威力",
        formula_hint: "显示=50×[1+(near_power-1)×0.25]×near_shotgun×1.1（本工具令二者=x）",
    },
    AxisDef {
        id: 2,
        name_zh: "射击威力",
        formula_hint: "显示值 ≈ 50 × far_power × 0.92  →  字段 = 显示值 / 46",
    },
    AxisDef {
        id: 3,
        name_zh: "锁定性能",
        formula_hint: "显示值 ≈ 50 × lockon_power  →  字段 = 显示值 / 50",
    },
    AxisDef {
        id: 4,
        name_zh: "魔力稳定性",
        formula_hint: "显示值 ≈ 50 × stability × mana_splash × 1.1 × overhold^0.75 × drain^0.5\n  （本工具只改 stability，其余保持当前读值）→ stability = 显示值 / 分母",
    },
    AxisDef {
        id: 5,
        name_zh: "魔力消耗效率",
        formula_hint: "目标<65：mp=sqrt(65/目标)；目标65–169：mp=(169-目标)/104",
    },
    AxisDef {
        id: 6,
        name_zh: "魔力亲和性",
        formula_hint: "显示值 ≈ 50 × neutral × 1.2  →  字段 = 显示值 / 60",
    },
    AxisDef {
        id: 7,
        name_zh: "咏唱速度",
        formula_hint: "简化：显示值 ≈ 50 × castspeed（magic_prepare/overhold 保持当前）→ castspeed = 显示值 / 50",
    },
    AxisDef {
        id: 8,
        name_zh: "近战挥击速度",
        formula_hint: "显示值 ≈ 50 × near_punch_speed  →  字段 = 显示值 / 50",
    },
];

const UI_DISPLAY_MAX: f32 = 255.0;

pub fn forward_display(axis_id: u8, f: &PrCaneFields) -> f32 {
    let raw = match axis_id {
        0 => 50.0 * f.near_reach,
        1 => 50.0 * (1.0 + (f.near_power - 1.0) * 0.25) * f.near_shotgun * 1.1,
        2 => 50.0 * f.far_power * 0.92,
        3 => 50.0 * f.lockon,
        4 => {
            50.0 * f.stability
                * f.mana_splash
                * 1.1
                * f.castspeed_overhold.powf(0.75)
                * f.drain_after_lock.powf(0.5)
        }
        5 => {
            if f.mp_use < 1.0 {
                169.0 - 104.0 * f.mp_use
            } else {
                65.0 / f.mp_use.powi(2)
            }
        }
        6 => 50.0 * f.neutral * 1.2,
        7 => 50.0 * f.castspeed,
        8 => 50.0 * f.near_punch,
        _ => 0.0,
    };
    raw.min(UI_DISPLAY_MAX)
}

pub fn invert_display(
    axis_id: u8,
    display: f32,
    base: &PrCaneFields,
) -> Result<PrCaneFields, String> {
    if !display.is_finite() || display <= 0.0 {
        return Err("显示值必须是正数".into());
    }
    let display = display.min(UI_DISPLAY_MAX);
    let mut out = *base;
    match axis_id {
        0 => {
            out.near_reach = display / 50.0;
        }
        1 => {
            let x = (-3.0 + (9.0 + 16.0 * display / 55.0).sqrt()) / 2.0;
            out.near_power = x;
            out.near_shotgun = x;
        }
        2 => {
            out.far_power = display / 46.0;
        }
        3 => {
            out.lockon = display / 50.0;
        }
        4 => {
            let denom = 50.0
                * base.mana_splash
                * 1.1
                * base.castspeed_overhold.powf(0.75)
                * base.drain_after_lock.powf(0.5);
            if denom.abs() < 1e-9 {
                return Err("稳定性分母过小，无法反算".into());
            }
            out.stability = display / denom;
        }
        5 => {
            if display > 169.0 {
                return Err("消耗效率公式上限约 169（UI 显示上限 255）".into());
            }
            out.mp_use = if display < 65.0 {
                (65.0 / display).sqrt()
            } else {
                (169.0 - display) / 104.0
            };
            if !out.mp_use.is_finite() || out.mp_use > 5.0 {
                return Err(format!(
                    "显示分 {display:.0} 太低（mp={:.2}>5.0），最低约 3",
                    out.mp_use
                ));
            }
            if out.mp_use < 0.05 {
                return Err(format!(
                    "显示分 {display:.0} 太高（mp={:.4}<0.05），最高约 164",
                    out.mp_use
                ));
            }
        }
        6 => {
            out.neutral = display / 60.0;
        }
        7 => {
            out.castspeed = display / 50.0;
        }
        8 => {
            out.near_punch = display / 50.0;
        }
        _ => return Err("未知轴".into()),
    }
    clamp_fields(&mut out);
    Ok(out)
}

fn clamp_field(v: &mut f32, lo: f32, hi: f32) {
    *v = if v.is_finite() { v.clamp(lo, hi) } else { 1.0 };
}

fn clamp_fields(f: &mut PrCaneFields) {
    for v in [
        &mut f.near_power,
        &mut f.near_shotgun,
        &mut f.near_reach,
        &mut f.near_punch,
        &mut f.magic_prepare,
        &mut f.drain_after_lock,
        &mut f.lockon,
        &mut f.far_power,
        &mut f.mana_splash,
        &mut f.stability,
        &mut f.neutral,
        &mut f.castspeed,
        &mut f.castspeed_overhold,
    ] {
        clamp_field(v, 0.05, 10.0);
    }
    clamp_field(&mut f.mp_use, 0.05, 5.0);
}

pub fn read_pr_fields(proc: &crate::mem::Process, pe: u64) -> Option<PrCaneFields> {
    Some(PrCaneFields {
        near_power: proc.read_f32(pe + PE_NEAR_POWER)?,
        near_shotgun: proc.read_f32(pe + PE_NEAR_SHOTGUN)?,
        near_reach: proc.read_f32(pe + PE_NEAR_REACH)?,
        near_punch: proc.read_f32(pe + PE_NEAR_PUNCH)?,
        magic_prepare: proc.read_f32(pe + PE_MAGIC_PREPARE)?,
        drain_after_lock: proc.read_f32(pe + PE_DRAIN_AFTER_LOCK)?,
        lockon: proc.read_f32(pe + PE_LOCKON)?,
        far_power: proc.read_f32(pe + PE_FAR_POWER)?,
        mana_splash: proc.read_f32(pe + PE_MANA_SPLASH)?,
        mp_use: proc.read_f32(pe + PE_MP_USE)?,
        stability: proc.read_f32(pe + PE_STABILITY)?,
        neutral: proc.read_f32(pe + PE_NEUTRAL)?,
        castspeed: proc.read_f32(pe + PE_CASTSPEED)?,
        castspeed_overhold: proc.read_f32(pe + PE_CASTSPEED_OVERHOLD)?,
    })
}

pub fn write_pr_axis(
    proc: &crate::mem::Process,
    pe: u64,
    axis_id: u8,
    f: &PrCaneFields,
) -> Result<(), String> {
    let pairs: &[(u64, f32)] = match axis_id {
        0 => &[(PE_NEAR_REACH, f.near_reach)],
        1 => &[
            (PE_NEAR_POWER, f.near_power),
            (PE_NEAR_SHOTGUN, f.near_shotgun),
        ],
        2 => &[(PE_FAR_POWER, f.far_power)],
        3 => &[(PE_LOCKON, f.lockon)],
        4 => &[(PE_STABILITY, f.stability)],
        5 => &[(PE_MP_USE, f.mp_use)],
        6 => &[(PE_NEUTRAL, f.neutral)],
        7 => &[(PE_CASTSPEED, f.castspeed)],
        8 => &[(PE_NEAR_PUNCH, f.near_punch)],
        _ => return Err("未知轴".into()),
    };
    for &(off, v) in pairs {
        if !proc.write_f32(pe + off, v) {
            return Err(format!("writeFloat pe+{off:#x} failed"));
        }
        crate::log::info(format!("写 pe+{off:#x} = {v:.6} (axis={axis_id})"));
    }
    Ok(())
}

pub fn write_cane_table_axis(
    proc: &crate::mem::Process,
    cane: u64,
    axis_id: u8,
    f: &PrCaneFields,
) -> Result<(), String> {
    let arrays: &[(u64, f32, f32)] = match axis_id {
        0 => &[(0x58, f.near_reach, f.near_reach)],
        1 => &[
            (CANE_NEAR_POWER, f.near_power, f.near_power * 1.15),
            (0x50, f.near_shotgun, f.near_shotgun * 1.15),
        ],
        2 => &[(0x70, f.far_power, f.far_power * 1.15)],
        3 => &[(0xB0, f.lockon, f.lockon * 1.1)],
        4 => &[(0x88, f.stability, f.stability * 1.1)],
        5 => &[(0x80, f.mp_use, (f.mp_use * 1.1).min(5.0))],
        6 => &[(0x90, f.neutral, f.neutral * 1.1)],
        7 => &[(0x98, f.castspeed, f.castspeed * 1.1)],
        8 => &[(0x60, f.near_punch, f.near_punch)],
        _ => return Err("未知轴".into()),
    };
    for &(off, lo, hi) in arrays {
        let arr = proc.read_u64(cane + off);
        let arr = match arr {
            Some(a) if a != 0 && proc.looks_like_user_ptr(a) => a,
            _ => {
                let template = proc.read_u64(cane + CANE_NEAR_POWER);
                match template {
                    Some(t) if t != 0 && proc.looks_like_user_ptr(t) => {
                        let mut header = [0u8; 32];
                        if !proc.read_bytes(t, &mut header) {
                            continue;
                        }
                        let n = u32::from_le_bytes(header[0x18..0x1C].try_into().unwrap()).max(2);
                        header[0x10..0x14].copy_from_slice(&n.to_le_bytes());
                        header[0x14..0x18].copy_from_slice(&n.to_le_bytes());
                        header[0x18..0x1C].copy_from_slice(&n.to_le_bytes());
                        let size = 0x20 + (n as usize) * 4;
                        let new_arr = match proc.alloc_remote(size) {
                            Some(a) => a,
                            None => continue,
                        };
                        if !proc.write_bytes(new_arr, &header) {
                            proc.free_remote(new_arr);
                            continue;
                        }
                        if !proc.write_u64(cane + off, new_arr) {
                            proc.free_remote(new_arr);
                            continue;
                        }
                        new_arr
                    }
                    _ => continue,
                }
            }
        };
        let n = proc.read_u32(arr + ARR_LEN).unwrap_or(1);
        if !proc.write_f32(arr + ARR_DATA, lo) {
            return Err(format!("write array[0] cane+{off:#x}"));
        }
        if n >= 2 && !proc.write_f32(arr + ARR_DATA + 4, hi) {
            return Err(format!("write array[1] cane+{off:#x}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_linear_roundtrip() {
        let base = PrCaneFields::default();
        let d = 75.0;
        let f = invert_display(0, d, &base).unwrap();
        let back = forward_display(0, &f);
        assert!((back - d).abs() < 0.01);
    }

    #[test]
    fn invert_near_power_grade0() {
        let base = PrCaneFields::default();
        let f = invert_display(1, 55.0, &base).unwrap();
        assert!((f.near_power - 1.0).abs() < 0.02);
        assert!((forward_display(1, &f) - 55.0).abs() < 0.5);
    }

    #[test]
    fn invert_near_power_linear_pow() {
        let base = PrCaneFields::default();
        let f = invert_display(1, 99.0, &base).unwrap();
        assert!((forward_display(1, &f) - 99.0).abs() < 0.01);
    }

    #[test]
    fn invert_mp_use() {
        let base = PrCaneFields::default();
        let f = invert_display(5, 65.0, &base).unwrap();
        assert!((f.mp_use - 1.0).abs() < 0.001);
        let f2 = invert_display(5, 3.0, &base).unwrap();
        assert!((forward_display(5, &f2) - 3.0).abs() < 0.001);
        let f3 = invert_display(5, 99.0, &base).unwrap();
        assert!((forward_display(5, &f3) - 99.0).abs() < 0.001);
    }

    #[test]
    fn ui_max_not_blocked_by_field_clamp() {
        let base = PrCaneFields::default();
        for axis in AXES {
            if axis.id == 5 {
                continue;
            }
            let f = invert_display(axis.id, 255.0, &base)
                .unwrap_or_else(|e| panic!("axis {} failed: {e}", axis.name_zh));
            let back = forward_display(axis.id, &f);
            assert!(
                (back - 255.0).abs() < 0.01,
                "axis {} expected 255, got {back}",
                axis.name_zh
            );
        }
    }
}
