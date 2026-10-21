#!/usr/bin/env node
/**
 * SBOM Generation Script
 *
 * Generates CycloneDX SBOMs for release artifacts.
 * Run after building Docker images and before publishing releases.
 */
const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const SBOM_DIR = path.join(process.cwd(), 'sbom');

function ensureDir(dir) {
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
}

function run(cmd) {
  console.log(`> ${cmd}`);
  try {
    return execSync(cmd, { encoding: 'utf-8', stdio: 'inherit' });
  } catch (err) {
    console.error(`Failed: ${cmd}`);
    process.exit(1);
  }
}

function generateCargoSBOM() {
  console.log('\n📦 Generating Cargo SBOM...');
  ensureDir(SBOM_DIR);

  // Generate SBOM for Rust workspace
  if (fs.existsSync('Cargo.lock')) {
    run(`cargo install cargo-cyclonedx --locked 2>/dev/null || true`);
    run(`cargo cyclonedx --format json --output-dir ${SBOM_DIR}/cargo`);
  }
}

function generateNpmSBOM() {
  console.log('\n📦 Generating npm SBOM...');
  ensureDir(SBOM_DIR);

  // Check for package.json in app directory
  const appDir = path.join(process.cwd(), 'app');
  if (fs.existsSync(path.join(appDir, 'package.json'))) {
    run(`cd ${appDir} && npx @cyclonedx/cyclonedx-npm --output-file ${path.join(SBOM_DIR, 'npm-app.json')} --output-format json`);
  }
}

function generateDockerSBOM(imageName) {
  console.log(`\n📦 Generating Docker SBOM for ${imageName}...`);
  ensureDir(SBOM_DIR);

  const safeName = imageName.replace(/[^a-z0-9]/gi, '-');
  run(`docker sbom ${imageName} --format cyclonedx-json --output ${SBOM_DIR}/docker-${safeName}.json 2>/dev/null || echo "Skipping Docker SBOM for ${imageName}"`);
}

function main() {
  console.log('🔐 StellPoker SBOM Generator');
  console.log('============================\n');

  // Generate SBOMs for each component
  generateCargoSBOM();
  generateNpmSBOM();

  // Docker image SBOMs (if images are built)
  const images = [
    'stellpoker-node',
    'stellpoker-coordinator',
    'stellpoker-app',
  ];

  for (const image of images) {
    generateDockerSBOM(image);
  }

  // Generate summary
  const sbomFiles = fs.readdirSync(SBOM_DIR).filter(f => f.endsWith('.json'));
  console.log('\n✅ Generated SBOMs:');
  for (const file of sbomFiles) {
    const stats = fs.statSync(path.join(SBOM_DIR, file));
    console.log(`   ${file} (${(stats.size / 1024).toFixed(1)}KB)`);
  }

  console.log(`\n📁 Output directory: ${SBOM_DIR}`);
}

main();
