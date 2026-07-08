# 旅行证件（eMRTD）读取分析

> 本文档记录在本项目（PC/SC + Rust）中实现电子旅行证件读取所需的协议流程、
> 密码学细节与字节布局。权威规范：ICAO Doc 9303（Part 10/11 为芯片与安全机制，
> Part 3/4 为 MRZ 与校验位）。
>
> 电子护照与电子往来港澳通行证共享同一套 ICAO 芯片机制（相同 AID、相同 BAC、
> 相同安全通道与 DG 文件布局），差异仅在 MRZ 的行列排布。故读取流程统一，
> 二者区别集中在末尾「按证件类型解析 MRZ」一节。

## 与普通卡的关键差异

普通交通卡/EMV 卡遵循「后端只抓原始字节、前端解析」的边界。旅行证件不同：

1. **需要外部密钥输入**：BAC 的密钥由 MRZ（机读区）三要素派生——
   证件号、出生日期、有效期。这些必须由用户提供，无法从芯片读出。
2. **有状态的加密会话**：BAC 成功后建立「安全消息通道」（Secure Messaging），
   之后每条 APDU 都要用会话密钥加密并计算 MAC，响应也要验签并解密。
   会话含一个随命令递增的 8 字节发送序列计数器（SSC）。
3. **加密会话必须在后端完成**：因为 SSC 递增与会话密钥是有状态的，
   与卡的每次往返都要即时加解密。因此后端负责 BAC + 安全通道 + 读 DG，
   把**解密后的 DG 明文字节**存入 `raw_fields`，前端只解析明文语义。

## 整体流程

```
1. SELECT eMRTD 应用   (AID = A0 00 00 02 47 10 01)
2. BAC 认证:
   a. 由 MRZ key 派生 Kenc / Kmac
   b. GET CHALLENGE -> 卡返回 8 字节随机数 rnd.icc
   c. 终端生成 rnd.ifd(8) 与 k.ifd(16)
   d. S = rnd.ifd || rnd.icc || k.ifd  (32 字节)
   e. Eifd = 3DES-CBC(Kenc, S, IV=0)
   f. Mifd = Retail-MAC(Kmac, pad(Eifd))
   g. MUTUAL AUTHENTICATE(Eifd || Mifd)  -> 卡返回 40 字节
   h. 验签并解密 -> rnd.icc' || rnd.ifd' || k.icc
   i. 校验 rnd 一致
   j. Kseed = k.ifd XOR k.icc ; 派生会话密钥 KSenc / KSmac
   k. SSC = rnd.icc[4..8] || rnd.ifd[4..8]
3. 通过安全通道读取各数据组(DG):
   对每个 DG: SELECT EF -> READ BINARY(分块) -> 去 SM 封装得明文
4. 前端解析明文 DG (DG1=MRZ, DG2=人脸, COM/SOD 等)
```

护照与港澳通行证在 1~3 步完全一致，后端统一以 `card_type = "TravelDoc"` 输出。

## MRZ 密钥

组成（ICAO 9303 Part 11 §9.7.1.2）：

```
MRZ_key = 证件号(9,不足补'<') + 校验位
        + 出生日期(YYMMDD)     + 校验位
        + 有效期(YYMMDD)       + 校验位
```

校验位算法（Part 3 §4.9）：字符按权重 `7,3,1` 循环加权求和后 mod 10。
字符取值：`0-9`→0-9，`A-Z`→10-35，`<` 及填充→0。

由 `core::PassportKey` 承载三要素并封装 `mrz_key()` 合成，供 backend / CLI / TUI 共用。

## 密钥派生（ICAO 9303 Part 11 §9.7.1）

```
Kseed  = SHA1(MRZ_key)[0..16]
派生:   input = Kseed || 00 00 00 c   (c=1 加密, c=2 MAC)
        H = SHA1(input)
        Ka = 奇校验(H[0..8]) ; Kb = 奇校验(H[8..16])
        Key = Ka || Kb        (16 字节 2-key 3DES)
```

奇校验：调整每字节最低位使高 7 位中 1 的个数为奇。

## Retail MAC（ISO 9797-1 Alg 3，3DES 模式用）

key 16 字节拆 Ka(8)/Kb(8)，消息须已按 Method 2 填充到 8 字节倍数：
```
1. DES-CBC(Ka, message, IV=0)   取最后 8 字节块 last
2. y = DES-ECB-Decrypt(Kb, last)
3. mac = DES-ECB-Encrypt(Ka, y)  (8 字节)
```

## ISO 9797-1 Method 2 填充

追加 `0x80`，再补 `0x00` 到块大小（3DES 为 8）倍数。去填充反之。

## 安全消息通道（BAC 后，3DES 模式）

每条命令前 SSC += 1，保护 APDU：
```
maskedCLA = CLA | 0x0C
cmdHeader = pad(maskedCLA INS P1 P2, 8)
若有数据: DO87 = 87 L (01 || 3DES-CBC(KSenc, pad(data), IV=0))
若有 Le : DO97 = 97 01 Le
MAC 输入 = pad(SSC || cmdHeader || DO87 || DO97, 8)
DO8E = 8E 08 RetailMAC(KSmac, MAC输入)
保护后数据 = DO87 || DO97 || DO8E, Le=0x00
```
响应前 SSC += 1，解保护：
```
解析 DO87(加密数据) / DO99(状态字) / DO8E(MAC)
验签: MAC(pad(SSC || DO87全TLV || DO99全TLV))  与 DO8E 比对
解密: 去 DO87 首字节(0x01)后 3DES-CBC 解密, 去 Method2 填充
状态字取自 DO99
```

## 数据组与文件 ID

护照与港澳通行证的 EF 文件 ID 一致。

| DG | EF FileID | 内容 |
|----|-----------|------|
| COM | 011E | 公共数据/DG 列表/LDS 版本 |
| DG1 | 0101 | MRZ（机读区文本） |
| DG2 | 0102 | 人脸图像（ISO 19794-5，JPEG/JP2） |
| DG7 | 0107 | 签名图像 |
| DG11 | 010B | 附加个人信息 |
| DG12 | 010C | 附加证件信息 |
| DG14 | 010E | 安全信息（CA/PACE） |
| DG15 | 010F | 主动认证公钥 |
| SOD | 011D | 安全对象（哈希与签名） |

本项目实际读取：COM、DG1、DG2、DG11、DG12、SOD。
EF.CardAccess（短 FID 01 1C）在 SELECT MF 后读，探测 PACE，可选。

### READ BINARY 分块

先读前 4 字节得 TLV tag+len，算出总长，再以 `00 B0 <offsetHi> <offsetLo> <Le>`
分块读（Le 上限保守取 0xA0=160）。读到 `6B00`（越界）或短读即停。

### DG1 (MRZ) 布局

```
61 L
  5F1F L  <MRZ ASCII 文本>
```
文本的行列排布因证件而异，详见文末「按证件类型解析 MRZ」。

### DG2 (人脸) 布局

嵌套 TLV：`7F61`→`7F60`→(`A1` 头 + `5F2E`/`7F2E` 生物数据)。
生物数据内部为 ISO 19794-5，含 JPEG 或 JPEG2000 图像，可按 magic 定位切出。

### DG11 (附加个人信息) 布局

```
6B L
  5C L <tag list>              标签清单
  5F0E L <全名，UTF-8 中文>     形如 E5BCA0E4B889 = "张三"
  5F0F L <拉丁转写全名>          形如 "ZHANG<<SAN"
```
注意：中文姓名为 UTF-8 编码，不是 GBK（与交通卡/身份证不同）。

### DG12 (附加证件信息) 布局

`6C` 包裹，可能仅含空的 `5C`/`5F1B` 占位。

### COM (公共数据) 布局

```
60 L
  5F01 L <LDS 版本，如 "0107">
  5F36 L <Unicode 版本，如 "040000">
  5C   L <DG 标签清单，如 60 61 75 6B 6C 77 6F>
```

## 按证件类型解析 MRZ

后端不区分证件类型；前端从 DG1 提取 MRZ 文本，去空白后按长度与文档码判定：

- 去空白后 88 字符 → 护照（TD3，2 行 × 44 列）；
- 去空白后 90 字符 → 往来港澳通行证（TD1 变体，中国排布，3 行 × 30 列）。

MRZ 通用原语（`parse/traveldoc/mrz.rs`）：填充符 `<`，去填充与分隔、
日期 `YYMMDD → YYYY-MM-DD`（世纪以 50 为阈值）、性别 `M/F` → 中文、
姓名以 `<<` 分隔为姓与名。

### 护照（TD3，`parse/traveldoc/passport.rs`）

两行各 44 字符：

```
行1: [0..2]文档码  [2..5]签发国  [5..44]姓<<名（'<'填充）
行2: [0..9]证件号  [9]校验  [10..13]国籍  [13..19]出生  [19]校验
     [20]性别  [21..27]有效期  [27]校验  [28..42]可选数据  ...
```

本项目取用：文档码、签发国、姓/名、证件号、国籍、出生日期、性别、有效期。
文档码首字符为 `P`。

### 往来港澳通行证（TD1 变体，`parse/traveldoc/eep.rs`）

中国特有排布，与 ICAO 标准 TD1 不同：证件号与出生/有效期同置于第 1 行，
姓名拉丁转写在第 2 行前置 12 个附加字符之后。三行各 30 字符：

```
行1: [0..2]文档码  [2..11]证件号  [11]校验  [12]填充
     [13..19]有效期  [19]校验  [20]填充
     [21..27]出生    [27]校验  [28]填充  [29]合成校验
行2: [0..12]附加字符  [12..30]姓<<名（拉丁转写）
行3: 中文姓名等附加信息（如有）
```

本项目取用：文档码、证件号、有效期、出生日期、姓/名。文档码首字符为 `C`。
中文姓名从 DG11 的 `5F0E`（UTF-8）读取，比 MRZ 拉丁转写更完整。

## 本项目的参数与集成设计

- **参数类型**：`core::PassportKey { doc_number, date_of_birth, date_of_expiry }`，
  提供 `mrz_key() -> String` 与 `validate()`，供 backend / CLI / TUI 共用。
- **后端**：`cards/traveldoc/` 实现 BAC + SM + 读 DG，入口 `read_traveldoc`。
  解密后的 DG 明文以 `passport_com/dg1/dg2/...` 键存入 `raw_fields`，
  `card_type = "TravelDoc"`。不入自动探测链，需上层显式调用。
- **CLI**：`cli traveldoc --doc-number <号> --dob <YYMMDD> --doe <YYMMDD> [--reader NAME]`。
- **TUI**：需输入三要素，填完触发读取。
- **前端解析**：`parse/traveldoc/` 按 MRZ 判定护照 / 通行证，
  分别解析对应布局，并从 DG2 提取人脸图像格式与大小。

## 真机验证

已用 ACR1251U PICC 接口对中国电子护照与电子往来港澳通行证实测通过：
SELECT 应用 → GET CHALLENGE → MUTUAL AUTHENTICATE 全部成功，
安全通道 SSC 递增/验签/解密正确，分块 READ BINARY 完整读出
COM、DG1、DG2、DG11、DG12、SOD。密码学实现（3DES/RetailMAC/派生）经真卡验证无误。
