"""
生成一个"答题演示"网页（纯前端，不需要装服务器，双击就能打开）。

这是"第一根柱子(出题)"通向"第二根柱子(答题+反馈)"的桥：
它从题库里取【已通过审核】的题，做成一个能答题、能看对错、能看解析和来源
知识点的网页。这正是报告 Phase 1 完成标志里的"可答题+可看对错"，也是
Phase 2 反馈功能要挂载的地方（现在解析是静态的；Phase 2 会把解析换成
LLM 基于知识库原文实时生成的过程层反馈）。

用法：
    python3 demo/build_demo.py
    # 生成 demo/quiz_static_offline.html，双击用浏览器打开即可
"""
import json
import os
import sqlite3

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()
OUT_HTML = os.path.join(BASE_DIR, "demo", "quiz_static_offline.html")


def load_approved():
    conn = connect_database()
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        "SELECT question_id, course_chapter, source_node_id, question_type, stem, "
        "options_json, answer, explanation, bloom_level, generation_model "
        "FROM questions WHERE review_status='已通过' ORDER BY course_chapter"
    ).fetchall()
    conn.close()
    out = []
    for r in rows:
        d = dict(r)
        opts = json.loads(d["options_json"]) if d["options_json"] else None
        # 概念题：选项是 [{text,is_correct,...}]；计算题：是步骤字符串数组
        if d["question_type"] == "单选" and opts and isinstance(opts[0], dict):
            options = [o.get("text") for o in opts]
        else:
            options = None  # 计算题不做选择，只给出参考答案
        out.append({
            "id": d["question_id"],
            "chapter": d["course_chapter"],
            "node": d["source_node_id"],
            "type": d["question_type"],
            "stem": d["stem"],
            "options": options,
            "answer": d["answer"],
            "explanation": d["explanation"] or "",
            "bloom": d["bloom_level"] or "",
            "model": d["generation_model"] or "",
        })
    return out


HTML_TEMPLATE = """<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>学-练-考 · 答题演示（Phase 1）</title>
<style>
  :root{ --ink:#1a2233; --sub:#6b7686; --line:#e6e9ef; --ok:#137a4b; --bad:#c0392b; --brand:#2d5bd7; --bg:#f6f7fb; }
  *{box-sizing:border-box}
  body{font-family:-apple-system,"Segoe UI",Roboto,"PingFang SC","Microsoft YaHei",sans-serif;
       margin:0;background:var(--bg);color:var(--ink);line-height:1.6}
  .wrap{max-width:760px;margin:0 auto;padding:24px 18px 80px}
  h1{font-size:20px;margin:8px 0 2px}
  .meta{color:var(--sub);font-size:13px;margin-bottom:18px}
  .bar{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:16px}
  select,button{font:inherit;padding:8px 12px;border:1px solid var(--line);border-radius:10px;background:#fff;color:var(--ink)}
  button{cursor:pointer}
  .card{background:#fff;border:1px solid var(--line);border-radius:16px;padding:20px;margin-bottom:16px;
        box-shadow:0 1px 2px rgba(20,30,60,.04)}
  .tag{display:inline-block;font-size:12px;color:var(--sub);background:#f0f2f7;border-radius:999px;padding:2px 10px;margin-right:6px}
  .stem{font-size:16px;font-weight:600;margin:10px 0 14px;white-space:pre-wrap}
  .opt{display:block;width:100%;text-align:left;padding:12px 14px;border:1px solid var(--line);
       border-radius:12px;background:#fff;margin:8px 0;transition:.15s}
  .opt:hover{border-color:var(--brand)}
  .opt.correct{border-color:var(--ok);background:#eafaf1}
  .opt.wrong{border-color:var(--bad);background:#fdecea}
  .opt:disabled{cursor:default;opacity:1}
  .feedback{margin-top:12px;padding:14px;border-radius:12px;background:#f7f9ff;border:1px solid #e2e8fb;font-size:14px}
  .feedback .ok{color:var(--ok);font-weight:700}
  .feedback .bad{color:var(--bad);font-weight:700}
  .src{color:var(--sub);font-size:12px;margin-top:8px}
  .calc pre{white-space:pre-wrap;background:#f7f9ff;border:1px solid #e2e8fb;border-radius:12px;padding:12px;font-size:13px}
  .foot{position:fixed;left:0;right:0;bottom:0;background:#fff;border-top:1px solid var(--line);
        padding:12px 18px;display:flex;justify-content:space-between;align-items:center}
  .foot .score{font-weight:700}
  .note{color:var(--sub);font-size:12px;margin-top:24px;border-top:1px dashed var(--line);padding-top:14px}
</style>
</head>
<body>
<div class="wrap">
  <h1>学-练-考 · 答题演示</h1>
  <div class="meta">Phase 1 · 只显示【已通过人工审核】的题目 · 共 <b id="total">0</b> 道</div>

  <div class="bar">
    <select id="chapterSel"><option value="">全部章节</option></select>
    <select id="typeSel">
      <option value="">全部题型</option>
      <option value="单选">只做单选</option>
      <option value="计算">只看计算题</option>
    </select>
    <button onclick="restart()">重新开始</button>
  </div>

  <div id="quiz"></div>

  <div class="note">
    这是"出题"通向"答题+反馈"的桥。现在的解析是出题时生成的静态文本；
    到 Phase 2，答错后的这段反馈会换成 LLM 基于对应知识点原文实时生成的
    "错因分析 + 学习建议"（报告第五节的过程层/自我调节层反馈）。
    每道题下方的"来源知识点"就是将来答错跳转复习的锚点。
  </div>
</div>

<div class="foot">
  <span class="score">得分：<span id="score">0</span> / <span id="answered">0</span></span>
  <span class="meta" id="progress"></span>
</div>

<script>
const DATA = __DATA__;
let pool = [], stats = {answered:0, correct:0};

function chapters(){ return [...new Set(DATA.map(q=>q.chapter))]; }
function initFilters(){
  const cs = document.getElementById('chapterSel');
  chapters().forEach(c=>{ const o=document.createElement('option'); o.value=c; o.textContent=c; cs.appendChild(o); });
  cs.onchange = restart; document.getElementById('typeSel').onchange = restart;
  document.getElementById('total').textContent = DATA.length;
}
function restart(){
  const ch = document.getElementById('chapterSel').value;
  const tp = document.getElementById('typeSel').value;
  pool = DATA.filter(q=>(!ch||q.chapter===ch) && (!tp||q.type===tp));
  stats = {answered:0, correct:0};
  render();
}
function render(){
  const box = document.getElementById('quiz'); box.innerHTML='';
  pool.forEach((q,i)=> box.appendChild(card(q,i)));
  updateScore();
}
function card(q,i){
  const el = document.createElement('div'); el.className='card';
  el.innerHTML = `<div><span class="tag">${q.chapter}</span><span class="tag">${q.type}</span>${q.bloom?`<span class="tag">Bloom:${q.bloom}</span>`:''}</div>
    <div class="stem">${i+1}. ${esc(q.stem)}</div>`;
  if(q.type==='单选' && q.options){
    q.options.forEach(opt=>{
      const b=document.createElement('button'); b.className='opt'; b.textContent=opt;
      b.onclick=()=>choose(el,q,opt,b); el.appendChild(b);
    });
  } else {
    // 计算题：给"显示参考答案与步骤"按钮
    const b=document.createElement('button'); b.className='opt'; b.textContent='显示参考答案与解析';
    b.onclick=()=>revealCalc(el,q,b); el.appendChild(b);
  }
  return el;
}
function choose(el,q,opt,btn){
  if(el.dataset.done) return; el.dataset.done=1;
  const correct = (opt===q.answer);
  el.querySelectorAll('.opt').forEach(b=>{
    b.disabled=true;
    if(b.textContent===q.answer) b.classList.add('correct');
    else if(b===btn) b.classList.add('wrong');
  });
  stats.answered++; if(correct) stats.correct++;
  const fb=document.createElement('div'); fb.className='feedback';
  fb.innerHTML = (correct?`<span class="ok">✔ 答对了</span>`:`<span class="bad">✘ 答错了</span> 正确答案：${esc(q.answer)}`)
    + `<div style="margin-top:8px">${esc(q.explanation)}</div>`
    + `<div class="src">来源知识点：${q.node}　·　出题模型：${q.model}</div>`;
  el.appendChild(fb); updateScore();
}
function revealCalc(el,q,btn){
  if(el.dataset.done) return; el.dataset.done=1; btn.disabled=true;
  const fb=document.createElement('div'); fb.className='feedback calc';
  let steps=''; try{ const s=JSON.parse(q.answer); }catch(e){}
  fb.innerHTML = `<div><b>参考答案：</b>${esc(q.answer)}</div>`
    + `<div style="margin-top:8px">${esc(q.explanation)}</div>`
    + `<div class="src">来源知识点：${q.node}　·　出题模型：${q.model}</div>`;
  el.appendChild(fb);
  stats.answered++; // 计算题不判分，只记作已查看
  updateScore();
}
function updateScore(){
  document.getElementById('score').textContent = stats.correct;
  document.getElementById('answered').textContent = stats.answered;
  document.getElementById('progress').textContent = `本次筛选共 ${pool.length} 道`;
}
function esc(s){ return String(s).replace(/[&<>]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;'}[c])); }
initFilters(); restart();
</script>
</body>
</html>"""


def main():
    data = load_approved()
    html = HTML_TEMPLATE.replace("__DATA__", json.dumps(data, ensure_ascii=False))
    os.makedirs(os.path.dirname(OUT_HTML), exist_ok=True)
    with open(OUT_HTML, "w", encoding="utf-8") as f:
        f.write(html)
    print(f"已生成答题演示网页：{OUT_HTML}")
    print(f"共放入 {len(data)} 道【已通过审核】的题。双击这个 html 文件即可在浏览器打开答题。")


if __name__ == "__main__":
    main()
