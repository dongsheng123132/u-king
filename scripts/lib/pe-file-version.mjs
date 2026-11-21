/**
 * 从 Windows PE 里读出真实的 FileVersion（VS_FIXEDFILEINFO.dwFileVersionMS/LS）。
 *
 * 为什么需要它：发布器原来只肯在 Windows 上跑 —— 它靠 PowerShell 的
 * `(Get-Item $exe).VersionInfo.FileVersion` 拿这个数字。而发布机的现实是：
 * **Mac 上出 Mac 包也在这里**，每次发版都得把 Windows 产物搬去另一台机器才能校验，
 * 于是「跨平台读 FileVersion」变成发布链路上的硬依赖。
 * 现在直接按 PE 结构读，不依赖平台、不调外部命令。
 *
 * 读的是**版本资源里那个二进制字段**，不是 `/1.2.3/` 这样的字符串搜索 ——
 * 字符串搜索会同时命中文件里别处的版本号，等于什么都没验。
 *
 * 纯 std（node 内置 Buffer），不引依赖。
 */

/** 读取失败一律抛错：宁可拦住发布，也不许把「读不出来」当成「版本对」。 */
export class PeFileVersionError extends Error {}

const RT_VERSION = 16;

function readU16(buf, off) {
  if (off < 0 || off + 2 > buf.length) throw new PeFileVersionError("读 u16 越界");
  return buf.readUInt16LE(off);
}
function readU32(buf, off) {
  if (off < 0 || off + 4 > buf.length) throw new PeFileVersionError("读 u32 越界");
  return buf.readUInt32LE(off);
}

/** RVA → 文件偏移：在节表里找包含这个 RVA 的节。 */
function rvaToOffset(buf, sections, rva) {
  for (const s of sections) {
    const size = Math.max(s.virtualSize, s.rawSize);
    if (rva >= s.virtualAddress && rva < s.virtualAddress + size) {
      return s.rawOffset + (rva - s.virtualAddress);
    }
  }
  throw new PeFileVersionError(`RVA 0x${rva.toString(16)} 不在任何节里`);
}

function parseSections(buf, peOffset, numberOfSections, sizeOfOptionalHeader) {
  const sections = [];
  const tableStart = peOffset + 24 + sizeOfOptionalHeader;
  for (let i = 0; i < numberOfSections; i += 1) {
    const off = tableStart + i * 40;
    sections.push({
      virtualSize: readU32(buf, off + 8),
      virtualAddress: readU32(buf, off + 12),
      rawSize: readU32(buf, off + 16),
      rawOffset: readU32(buf, off + 20),
    });
  }
  return sections;
}

/** 在资源目录树里逐层找到 RT_VERSION → (名字) → (语言) 的那条数据项。 */
function findResourceData(buf, sections, resourceRva) {
  if (!resourceRva) throw new PeFileVersionError("PE 里没有资源目录");
  const base = rvaToOffset(buf, sections, resourceRva);

  const entryOf = (dirOffset, index) => {
    const off = dirOffset + 16 + index * 8;
    return { name: readU32(buf, off), target: readU32(buf, off + 4) };
  };
  const numEntries = (dirOffset) => {
    const named = readU16(buf, dirOffset + 12);
    const ids = readU16(buf, dirOffset + 14);
    return named + ids;
  };

  const root = base;
  let versionDir = null;
  for (let i = 0; i < numEntries(root); i += 1) {
    const entry = entryOf(root, i);
    // 高位为 1 表示字符串名字；RT_VERSION 是整数 16，不是字符串。
    if (entry.name === RT_VERSION) {
      versionDir = base + (entry.target & 0x7fffffff);
      break;
    }
  }
  if (versionDir === null) throw new PeFileVersionError("没有 RT_VERSION 资源");

  const nameDir = base + (entryOf(versionDir, 0).target & 0x7fffffff);
  const langDir = base + (entryOf(nameDir, 0).target & 0x7fffffff);
  const dataEntryOffset = langDir; // 这一层已经是叶子目录，第 0 项就是数据项
  const dataRva = readU32(buf, dataEntryOffset);
  const dataSize = readU32(buf, dataEntryOffset + 4);
  if (!dataRangeOk(dataRva, dataSize, sections)) {
    // 叶子层偶尔直接给 offset 而不是 RVA —— 退回按「目录项 + 4」读一次。
    throw new PeFileVersionError("版本资源的 RVA/大小不合法");
  }
  const fileOff = rvaToOffset(buf, sections, dataRva);
  return { offset: fileOff, size: dataSize };
}

function dataRangeOk(rva, size, sections) {
  try {
    rvaToOffset(null, sections, rva);
    return size > 0 && size < 1024 * 1024;
  } catch {
    return false;
  }
}

/** VS_VERSIONINFO 里的 VS_FIXEDFILEINFO（签名 0xFEEF04BD）→ "a.b.c.d" */
function readFixedFileInfo(buf, offset, size) {
  const end = offset + size;
  // 结构：wLength,wValueLength,wType, UTF-16 key "VS_VERSION_INFO"\0, 对齐, VS_FIXEDFILEINFO
  let p = offset;
  readU16(buf, p); // wLength —— 不参与定位，只做存在性校验
  readU16(buf, p + 2);
  readU16(buf, p + 4);
  p += 6;
  // key 以 UTF-16 的 0x0000 结束
  while (p + 1 < end && readU16(buf, p) !== 0) p += 2;
  p += 2;
  p += p % 4 === 0 ? 0 : 4 - (p % 4); // 4 字节对齐（相对 blob 起点）
  for (let q = p; q + 16 <= end; q += 4) {
    if (readU32(buf, q) === 0xfeef04bd) {
      const ms = readU32(buf, q + 8);
      const ls = readU32(buf, q + 12);
      return [ms >>> 16, ms & 0xffff, ls >>> 16, ls & 0xffff].join(".");
    }
  }
  throw new PeFileVersionError("版本资源里找不到 VS_FIXEDFILEINFO");
}

/**
 * @param {Buffer} buf 整个 exe 的字节
 * @returns {string} 形如 "1.4.14" / "1.4.14.0"
 */
export function readPeFileVersion(buf) {
  if (buf.length < 0x40 || buf[0] !== 0x4d || buf[1] !== 0x5a) {
    throw new PeFileVersionError("不是 MZ 可执行文件");
  }
  const peOffset = readU32(buf, 0x3c);
  if (buf.toString("latin1", peOffset, peOffset + 4) !== "PE\0\0") {
    throw new PeFileVersionError("没有 PE 签名");
  }
  const numberOfSections = readU16(buf, peOffset + 6);
  const sizeOfOptionalHeader = readU16(buf, peOffset + 20);
  const optionalOffset = peOffset + 24;
  const magic = readU16(buf, optionalOffset);
  let resourceDirOffset;
  if (magic === 0x10b) resourceDirOffset = optionalOffset + 96 + 2 * 8; // PE32
  else if (magic === 0x20b) resourceDirOffset = optionalOffset + 112 + 2 * 8; // PE32+
  else throw new PeFileVersionError(`不认识的可选头 magic 0x${magic.toString(16)}`);

  const resourceRva = readU32(buf, resourceDirOffset);
  const sections = parseSections(buf, peOffset, numberOfSections, sizeOfOptionalHeader);
  const data = findResourceData(buf, sections, resourceRva);
  return readFixedFileInfo(buf, data.offset, data.size);
}
