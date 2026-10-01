import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { verifySignature, signatureKeyId } from "./verify-desktop-signatures.mjs";

test("actual minisign signatures reject wrong roots and altered payloads", t => {
  const work=mkdtempSync(path.join(tmpdir(), "desktop-minisign-fixture-"));
  t.after(()=>rmSync(work,{recursive:true,force:true}));
  for(const key of ["first","second"]) execFileSync("minisign", ["-G","-W","-p",path.join(work,key+".pub"),"-s",path.join(work,key+".key")],{stdio:"pipe"});
  const file=path.join(work,"payload"); writeFileSync(file,"controlled updater bytes");
  execFileSync("minisign", ["-S","-s",path.join(work,"first.key"),"-m",file],{stdio:"pipe"});
  const sig=readFileSync(file+".minisig").toString("base64");
  const first=readFileSync(path.join(work,"first.pub")).toString("base64");
  const second=readFileSync(path.join(work,"second.pub")).toString("base64");
  assert.match(signatureKeyId(sig),/^[A-F0-9]{16}$/u);
  assert.doesNotThrow(()=>verifySignature(file,sig,first));
  assert.throws(()=>verifySignature(file,sig,second));
  writeFileSync(file,"tampered updater bytes");
  assert.throws(()=>verifySignature(file,sig,first));
});
