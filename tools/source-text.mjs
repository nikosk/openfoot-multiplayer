// English message strings from pinned OpenFoot Manager 64677fee.
// Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
import {readFileSync} from 'node:fs';
const catalog=JSON.parse(readFileSync(new URL('../data/source-messages-en.json',import.meta.url),'utf8'));
export function sourceText(key,params={}) {
  const value=String(key).split('.').reduce((node,part)=>node&&Object.hasOwn(node,part)?node[part]:undefined,catalog);
  if(typeof value!=='string')return key;
  return value.replace(/\{\{\s*([^{}]+?)\s*\}\}/g,(match,name)=>Object.hasOwn(params,name)?String(params[name]):match);
}
// Only render keys already present in the authorized projection. Never fetch
// private context, add strategic advice, or mutate the underlying message.
export function renderSourceText(value,inherited={}) {
  if(Array.isArray(value))return value.map(item=>renderSourceText(item,inherited));
  if(!value||typeof value!=='object')return value;
  const params={...inherited,...value.i18n_params,...value.note_params,...value.params};
  const result=Object.fromEntries(Object.entries(value).map(([key,item])=>[key,renderSourceText(item,params)]));
  for(const key of ['subject','headline','source','body','sender','sender_role','label','description','note']) {
    if(typeof value[`${key}_key`]==='string')result[key]=sourceText(value[`${key}_key`],params);
  }
  return result;
}
