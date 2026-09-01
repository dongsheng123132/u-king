// origin-kernel · Ed25519 签名（零依赖，node:crypto 自带）
// 用途：Lease 与 Receipt 的签发/验签。第三方只拿公钥即可离线验证。
import { generateKeyPairSync, sign as cryptoSign, verify as cryptoVerify, createHash } from 'node:crypto';

export function keygen(seedName = 'origin-key') {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  return {
    name: seedName,
    priv: privateKey.export({ type: 'pkcs8', format: 'pem' }).toString(),
    pub: publicKey.export({ type: 'spki', format: 'pem' }).toString(),
  };
}

export function sha256(obj) {
  return createHash('sha256').update(canonical(obj)).digest('hex');
}

export function sign(privPem, obj) {
  return cryptoSign(null, Buffer.from(canonical(obj)), privPem).toString('base64');
}

export function verify(pubPem, obj, sigB64) {
  try {
    return cryptoVerify(null, Buffer.from(canonical(obj)), pubPem, Buffer.from(sigB64, 'base64'));
  } catch {
    return false;
  }
}

// 确定性序列化：键排序，保证同对象哈希一致
function canonical(value) {
  if (value === null || typeof value !== 'object') return JSON.stringify(value ?? null);
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']';
  const keys = Object.keys(value).sort();
  return '{' + keys.map((k) => JSON.stringify(k) + ':' + canonical(value[k])).join(',') + '}';
}
