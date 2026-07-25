const colors = { Chapter: '#2563eb', Section: '#16a34a', Knowledge: '#f59e0b', Concept: '#94a3b8' };
const nodes = new Map();
const edges = new Map();
const expanded = new Set();
const branches = new Map();

const cy = cytoscape({
  container: document.getElementById('cy'), elements: [],
  style: [
    { selector: 'node', style: { 'background-color': 'data(color)', label: 'data(label)', 'font-size': 11, 'text-valign': 'bottom', 'text-margin-y': 7, color: '#26354d', width: 34, height: 34, 'border-width': 2, 'border-color': '#fff', 'text-wrap': 'ellipsis', 'text-max-width': 110 } },
    { selector: 'node:selected', style: { 'border-color': '#246bfd', 'border-width': 4 } },
    { selector: 'edge', style: { width: 1.5, 'line-color': '#a8b4c5', 'target-arrow-color': '#a8b4c5', 'target-arrow-shape': 'triangle', 'curve-style': 'bezier', label: 'data(label)', 'font-size': 8, color: '#78869a', 'text-background-color': '#f8fafc', 'text-background-opacity': .9 } },
    { selector: '.expanded', style: { 'border-color': '#16a085' } }
  ], layout: { name: 'cose', animate: false }
});

function nodeOf(raw) {
  if (!raw || !raw.elementId) return null;
  const labels = raw.labels || ['Entity'];
  const type = ['Chapter', 'Section', 'Knowledge', 'Concept'].find(x => labels.includes(x)) || 'Entity';
  return { data: { id: raw.elementId, businessId: raw.id, label: raw.name || type, type, color: colors[type] || '#64748b', raw } };
}
function edgeOf(raw) {
  if (!raw || !raw.elementId || !raw.startNodeElementId || !raw.endNodeElementId) return null;
  return { data: { id: raw.elementId, source: raw.startNodeElementId, target: raw.endNodeElementId, label: raw.type || '', raw } };
}
function scan(value, outN = [], outE = []) {
  if (Array.isArray(value)) { value.forEach(v => scan(v, outN, outE)); return { nodes: outN, edges: outE }; }
  if (value && typeof value === 'object') {
    if (value.labels && value.elementId) outN.push(value);
    else if (value.startNodeElementId && value.elementId) outE.push(value);
    else Object.values(value).forEach(v => scan(v, outN, outE));
  }
  return { nodes: outN, edges: outE };
}
function layout() { cy.layout({ name: 'cose', animate: true, animationDuration: 350, padding: 45 }).run(); }
function merge(payload, replace = false) {
  const found = scan(payload), addedNodeIds = [], addedEdgeIds = [];
  if (replace) { nodes.clear(); edges.clear(); expanded.clear(); branches.clear(); cy.elements().remove(); }
  found.nodes.map(nodeOf).filter(Boolean).forEach(n => {
    if (!nodes.has(n.data.id)) { nodes.set(n.data.id, n); cy.add(n); addedNodeIds.push(n.data.id); }
  });
  found.edges.map(edgeOf).filter(Boolean).forEach(e => {
    if (!edges.has(e.data.id) && nodes.has(e.data.source) && nodes.has(e.data.target)) {
      edges.set(e.data.id, e); cy.add(e); addedEdgeIds.push(e.data.id);
    }
  });
  layout(); updateCounts();
  return { nodeIds: addedNodeIds, edgeIds: addedEdgeIds };
}
function updateCounts() { document.getElementById('counts').textContent = `${nodes.size} 个节点 / ${edges.size} 条关系`; }
async function api(path, options) {
  const response = await fetch(path, options);
  const text = await response.text();
  let data = {};
  if (text) {
    try { data = JSON.parse(text); }
    catch (_) { throw new Error(`服务器返回异常（HTTP ${response.status}）：${text.slice(0, 100)}`); }
  }
  if (!response.ok) throw new Error(data.detail || `请求失败（HTTP ${response.status}）`);
  return data;
}
async function loadDemo() {
  msg('正在加载七个章节…');
  const data = await api('/api/process-graph/chapters');
  merge(data, true); msg('单击章节开始逐层展开；再次单击可收起');
}
async function expand(ele) {
  const businessId = ele.data('businessId');
  if (expanded.has(businessId)) { collapse(ele); return; }
  msg('正在加载下一层…');
  const data = await api('/api/process-graph/expand', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ element_id: businessId }) });
  const branch = merge(data.results || data);
  branches.set(businessId, branch); expanded.add(businessId); ele.addClass('expanded');
  msg('已展开；再次点击此节点可收起');
}
function collapse(ele) {
  const businessId = ele.data('businessId'), branch = branches.get(businessId);
  if (!branch) return;
  branch.nodeIds.forEach(id => {
    const child = cy.getElementById(id);
    if (child.length && expanded.has(child.data('businessId'))) collapse(child);
  });
  branch.edgeIds.forEach(id => { cy.getElementById(id).remove(); edges.delete(id); });
  branch.nodeIds.forEach(id => { cy.getElementById(id).remove(); nodes.delete(id); });
  branches.delete(businessId); expanded.delete(businessId); ele.removeClass('expanded');
  layout(); updateCounts(); msg('已收起该节点的下级知识树');
}
async function search() {
  const q = document.getElementById('search').value.trim(); if (!q) return;
  msg('搜索中…');
  const data = await api('/api/process-graph/search', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ query: q, limit: 20 }) });
  merge(data, true); msg(data.nodes.length ? `找到 ${data.nodes.length} 条结果` : '未找到结果');
}
function msg(t) { document.getElementById('message').textContent = t; }
async function showDetails(ele) {
  const raw = ele.data('raw'), type = ele.data('type');
  if (type === 'Knowledge') {
    const d = await api(`/api/process-graph/knowledge/${encodeURIComponent(raw.id)}`);
    document.getElementById('details').innerHTML = `<div class="prop"><b>知识点名称：</b>${d.name}</div><div class="prop"><b>所属章节：</b>${d.chapter}</div><div class="prop"><b>所属小节：</b>${d.section}</div><div class="prop"><b>知识内容：</b>${d.content}</div>`;
    return;
  }
  document.getElementById('details').innerHTML = `<div class="prop"><b>节点类型：</b>${type}</div><div class="prop"><b>名称：</b>${raw.name}</div>`;
}
cy.on('tap', 'node', e => {
  const ele = e.target, type = ele.data('type');
  showDetails(ele).catch(showError);
  if (type === 'Chapter' || type === 'Section') expand(ele).catch(showError);
});
cy.on('dbltap', 'node', e => { if (e.target.data('type') === 'Knowledge') expand(e.target).catch(showError); });
document.getElementById('searchBtn').onclick = () => search().catch(showError);
document.getElementById('search').onkeydown = e => { if (e.key === 'Enter') search().catch(showError); };
document.getElementById('resetBtn').onclick = () => loadDemo().catch(showError);
function showError(e) { msg(`错误：${e.message}`); }
api('/health').then(h => {
  const s = document.getElementById('status');
  s.textContent = h.neo4j ? 'Neo4j 已连接' : 'Neo4j 未连接'; s.className = `status ${h.neo4j ? 'ok' : 'error'}`;
  if (!h.neo4j) throw new Error('Neo4j 服务未运行，请先执行 start.ps1');
  return loadDemo();
}).catch(showError);
