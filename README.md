# aic-tools

aic 神秘内存小工具

下载即用，无额外依赖库（目前仅 Windows；没有 Mac 设备，暂无法调试）。解析、定位、修改等流程均自行实现(不依赖CE)

## 功能

### 1. 修改法杖数值

- 对初学者法杖写入九维属性（未填写的数值则保持原值）
- UI 上限 255 别当真
- 如果填入的数值过大可能有计算误差

### 2. 诺艾尔汁 → 法杖雷达 UI

- 让诺艾尔汁详情页显示法杖九维雷达图，仅改详情显示，不影响物品实际功能

### 3. 禁用动态马赛克

- 运行时 JIT patch `nel.MosaicShower.FnDrawMosaic` 入口为 `xor eax,eax; ret`
- 覆盖立绘 / 自慰动画 / Cut-in 等经 `MosaicShower` 的动态打码
- 静态码去不掉，哈酱直接画上面的

## 构建

```powershell
cd aic-tools
cargo build --release
# 产物: target\release\aic-tools.exe
```

静态 MSVC 目标（与 CI 一致）：

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

## 使用

1. 启动游戏并进入存档  
   - 装备初学者法杖  
   - 诺艾尔汁需已加载  
   - 马赛克：任意可触发敏感动画的场景即可
2. **管理员**运行 `aic-tools.exe`

## 注意事项

- 仅支持 Alice In Cradle **0.29j**
- 请先备份存档：`%USERPROFILE%\AppData\LocalLow\NanameHacha\AliceInCradle\`
- 内存补丁仅当前进程有效，重启游戏后失效

## 许可

MIT
