// Operator-only launcher helper. Never exposed through Workshop's agent API.
import fs from 'node:fs';
import path from 'node:path';

const [operation, dataRoot, instance, source] = process.argv.slice(2);
if (!['enable', 'disable', 'status'].includes(operation) || !path.isAbsolute(dataRoot ?? '') || !instance) {
  throw new Error('usage: qa-policy.mjs enable|disable|status ABSOLUTE_DATA_ROOT INSTANCE [PROFILE]');
}
const target = path.join(dataRoot, 'qa-policy.json');
if (operation === 'enable') {
  const bytes = fs.readFileSync(source);
  const profile = JSON.parse(bytes);
  const fields = ['schema_version','id','instance','expires_at','container_roots','recipe_roots',
    'containers','recipes','inline_evaluation_digests','providers','proxy_lease_providers','max_request_usd_micros',
    'max_total_usd_micros','max_rollouts'];
  if (Object.keys(profile).some(key => !fields.includes(key))) throw new Error('Unknown QA profile field');
  for (const key of ['container_roots','recipe_roots','containers','recipes','providers']) {
    if (!Array.isArray(profile[key]) || !profile[key].every(value=>typeof value === 'string')) {
      throw new Error(`Invalid QA profile ${key}`);
    }
  }
  for (const key of ['max_request_usd_micros','max_total_usd_micros','max_rollouts']) {
    if (!Number.isSafeInteger(profile[key])) throw new Error(`Invalid integer ${key}`);
  }
  if (profile.proxy_lease_providers !== undefined && (!Array.isArray(profile.proxy_lease_providers)
      || !profile.proxy_lease_providers.every(p => profile.providers.includes(p)))) {
    throw new Error('Proxy lease providers must be explicitly allowed providers');
  }
  if (profile.schema_version !== 1 || profile.instance !== instance || !(Date.parse(profile.expires_at) > Date.now())) {
    throw new Error('QA profile must name this instance and have a future expiry');
  }
  if (!(profile.max_request_usd_micros > 0 && profile.max_request_usd_micros <= profile.max_total_usd_micros
      && profile.max_total_usd_micros < 50_000_000 && profile.max_rollouts > 0)) {
    throw new Error('Invalid QA budget bounds');
  }
  fs.mkdirSync(dataRoot, {recursive: true});
  // Refuse silent replacement of an existing authorization envelope.
  fs.writeFileSync(target, bytes, {flag: 'wx', mode: 0o600});
  console.log(`QA policy ${profile.id} installed for ${instance}; runtime validates all scopes before use.`);
} else if (operation === 'disable') {
  if (fs.existsSync(target)) {
    const archived = `${target}.revoked-${Date.now()}`;
    fs.renameSync(target, archived);
    console.log(`QA policy revoked; preserved at ${archived}. Budget history retained.`);
  } else console.log('QA policy is not installed.');
} else {
  console.log(fs.existsSync(target) ? fs.readFileSync(target, 'utf8') : 'QA policy is not installed.');
}
