import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const readJson = (path) => JSON.parse(readFileSync(path, 'utf8'))

function fixture(t) {
  const directory = mkdtempSync(join(tmpdir(), 'toolbox-versioning-'))
  assert.equal(dirname(directory), resolve(tmpdir()))
  t.after(() => rmSync(directory, { recursive: true, force: true }))
  mkdirSync(join(directory, 'scripts'))
  mkdirSync(join(directory, 'src-tauri', 'src'), { recursive: true })
  for (const path of [
    'package.json',
    'package-lock.json',
    'src-tauri/Cargo.toml',
    'src-tauri/Cargo.lock',
    'src-tauri/tauri.conf.json',
    'scripts/build-and-install.ps1'
  ]) {
    cpSync(join(repository, path), join(directory, path))
  }
  writeFileSync(join(directory, 'src-tauri', 'src', 'lib.rs'), '')
  return directory
}

function bump(directory) {
  return spawnSync(
    'pwsh',
    [
      '-NoProfile',
      '-File',
      join(directory, 'scripts', 'build-and-install.ps1'),
      '-Bump',
      'patch',
      '-NoBuild'
    ],
    {
      cwd: directory,
      encoding: 'utf8',
      timeout: 120_000,
      windowsHide: true
    }
  )
}

test('version bumps synchronize manifests and retain dependency locks', (t) => {
  const directory = fixture(t)
  const before = readJson(join(directory, 'package.json'))
  const parts = before.version.split('.').map(Number)
  parts[2] += 1
  const version = parts.join('.')
  const expectedNpmLock = readJson(join(directory, 'package-lock.json'))
  expectedNpmLock.version = version
  expectedNpmLock.packages[''].version = version
  const cargoLock = readFileSync(
    join(directory, 'src-tauri', 'Cargo.lock'),
    'utf8'
  ).replaceAll('\r\n', '\n')
  const expectedCargoLock = cargoLock.replace(
    `name = "utility-desktop"\nversion = "${before.version}"`,
    `name = "utility-desktop"\nversion = "${version}"`
  )
  assert.ok(
    expectedCargoLock !== cargoLock,
    'fixture must contain the workspace lock entry'
  )

  const result = bump(directory)
  assert.equal(
    result.status,
    0,
    result.error?.message ?? result.stdout + result.stderr
  )
  assert.equal(readJson(join(directory, 'package.json')).version, version)
  assert.equal(
    readJson(join(directory, 'src-tauri', 'tauri.conf.json')).version,
    version
  )
  assert.match(
    readFileSync(join(directory, 'src-tauri', 'Cargo.toml'), 'utf8'),
    new RegExp(`^version = "${version.replaceAll('.', '\\.')}"`, 'm')
  )
  assert.deepEqual(
    readJson(join(directory, 'package-lock.json')),
    expectedNpmLock
  )
  assert.ok(
    readFileSync(join(directory, 'src-tauri', 'Cargo.lock'), 'utf8').replaceAll(
      '\r\n',
      '\n'
    ) === expectedCargoLock,
    'Cargo dependency entries must remain unchanged'
  )
})

test('a lockfile failure stops release work and restores version files', (t) => {
  const directory = fixture(t)
  writeFileSync(join(directory, 'src-tauri', 'Cargo.lock'), 'invalid lockfile')
  const paths = [
    'package.json',
    'package-lock.json',
    'src-tauri/Cargo.toml',
    'src-tauri/Cargo.lock',
    'src-tauri/tauri.conf.json'
  ]
  const originals = paths.map((path) => readFileSync(join(directory, path)))
  const result = bump(directory)
  assert.notEqual(result.status, 0, result.stdout + result.stderr)
  assert.match(result.stderr, /Cargo.lock version update failed/)
  paths.forEach((path, index) => {
    assert.deepEqual(
      readFileSync(join(directory, path)),
      originals[index],
      `${path} must be restored`
    )
  })
})
