# PPT-MASTER 生成过程

本文档用于说明 `ppt-master` 如何从源材料生成最终的 `PPTX` 文件，以及各阶段的输入、输出、关键脚本和执行原则。

## 1. 工作流总览

`ppt-master` 的核心链路如下：

`源材料 -> 项目初始化 -> 模板选择（可选） -> 策划与确认 -> 配图生成（可选） -> SVG 页面执行 -> 质量检查 -> 后处理 -> 导出 PPTX`

这个流程不是自由跳转的松散步骤，而是严格串行执行的流水线。前一步的产物会成为后一步的输入，因此不能随意跨阶段生成内容。

## 2. 执行原则

- 必须先阅读 `skills/ppt-master/SKILL.md`，它是项目内的权威流程定义。
- 整个流程默认串行执行，不能把后续阶段提前“预生成”。
- 某些步骤需要用户确认后才能继续，不能替用户做决定。
- `Executor` 阶段的 SVG 页面必须逐页生成，不能批量脚本化生成全部页面。
- 每一页生成前都要重新参考项目内的 `spec_lock.md`，确保颜色、字体、图标、图片和页面节奏一致。

## 3. 阶段拆解

### 3.1 源材料处理

目标是把输入资料统一整理成后续可消费的文本与素材。

支持的输入包括：

- PDF
- DOCX / Word
- XLSX / XLSM
- PPTX
- URL / 网页
- Markdown
- 用户直接在对话中提供的文字

常用脚本：

```bash
python3 skills/ppt-master/scripts/source_to_md/pdf_to_md.py <PDF_file>
python3 skills/ppt-master/scripts/source_to_md/doc_to_md.py <DOCX_or_other_file>
python3 skills/ppt-master/scripts/source_to_md/excel_to_md.py <XLSX_or_XLSM_file>
python3 skills/ppt-master/scripts/source_to_md/ppt_to_md.py <PPTX_file>
python3 skills/ppt-master/scripts/source_to_md/web_to_md.py <URL>
```

阶段输出通常是：

- 转换后的 Markdown
- 从源文件中提取出的图片与资源文件
- 可以被导入项目目录的标准化内容

如果用户只给了主题，没有给任何实际材料，则要先走专题调研流程，再回到主流程。

### 3.2 项目初始化

目标是创建标准项目目录，为后续内容、图片、SVG、备注和导出文件提供固定结构。

常用命令：

```bash
python3 skills/ppt-master/scripts/project_manager.py init <project_name> --format ppt169
python3 skills/ppt-master/scripts/project_manager.py import-sources <project_path> <source_files_or_URLs...> --move
python3 skills/ppt-master/scripts/project_manager.py validate <project_path>
```

项目目录一般会包含：

- `sources/`：原始材料与转换结果
- `images/`：配图与视觉资产
- `notes/`：讲稿与备注
- `svg_output/`：原始 SVG 页面
- `svg_final/`：后处理后的 SVG 页面
- `templates/`：项目内模板资源
- `*.pptx`：最终导出的演示文稿

### 3.3 模板选择或自由设计

这一阶段是可选的。

- 如果用户明确提供了模板目录路径，则将其复制到项目内 `templates/`
- 如果没有明确路径，则默认走自由设计

模板可能是三类：

- `brand`：品牌身份规范，如色板、字体、Logo、语气
- `layout`：结构模板，如页面布局、页面类型、版式骨架
- `deck`：完整模板，包含身份和结构

如果用户提供多个模板路径，还需要按规则做融合，决定谁覆盖品牌层、谁覆盖结构层。

### 3.4 策划与确认

这一阶段由 `Strategist` 主导，负责把源材料变成可执行的演示设计方案。

主要工作包括：

- 明确受众、目标和使用场景
- 确定页数范围与内容深度
- 制定章节结构与页面大纲
- 确认视觉方向、风格、品牌限制
- 产出 `design_spec.md`
- 进一步锁定执行规范到 `spec_lock.md`

这里通常会有需要用户明确确认的内容。确认完成后，后续非阻塞步骤可以连续推进。

### 3.5 配图与视觉资源生成

如果页面需要插图、概念图、场景图或补充视觉素材，会在这一阶段生成。

常用脚本：

```bash
python3 skills/ppt-master/scripts/analyze_images.py <project_path>/images
python3 skills/ppt-master/scripts/image_gen.py --manifest <project_path>/images/image_prompts.json
python3 skills/ppt-master/scripts/image_gen.py --render-md <project_path>/images/image_prompts.json
```

这一阶段的输入通常来自：

- `Strategist` 写好的图像需求
- 现有源文件提取出的图片
- 品牌或模板附带的素材

输出通常包括：

- 实际生成的图片文件
- 图像清单与分析结果

### 3.6 SVG 页面执行

这一阶段由 `Executor` 执行，是主流程里最核心的一步。

关键规则：

- 页面必须逐页生成
- 不能用脚本循环批量吐出整套 SVG
- 每页都要参考 `spec_lock.md`
- 页面节奏、模板继承、图表模板适配，都要按当前页配置执行

这一阶段的目标是得到高质量、可控、视觉一致的 SVG 页面。每一页都相当于 PPT 中的一张幻灯片草图，但它已经具备很强的版式与视觉确定性。

如果需要看效果或做交互式检查，可以启用 live preview 工作流。

### 3.7 质量检查

在 SVG 页面初步完成后，需要做质量检查，确保导出前没有明显结构或视觉问题。

常用命令：

```bash
python3 skills/ppt-master/scripts/svg_quality_checker.py <project_path>
```

重点检查内容通常包括：

- 页面是否缺元素
- 文本是否溢出
- 图片引用是否正确
- 样式是否统一
- 结构是否符合导出要求

如果页面中含有数据图表，可能还要插入额外的图表校准流程。

### 3.8 后处理

后处理用于把页面资源整理成最终适合导出的状态。

主流程要求按顺序执行，不能并成一条命令混跑：

```bash
python3 skills/ppt-master/scripts/total_md_split.py <project_path>
python3 skills/ppt-master/scripts/finalize_svg.py <project_path>
```

这一步通常会完成：

- 讲稿与备注拆分整理
- SVG 清理与标准化
- 为导出阶段准备最终页面输入

### 3.9 导出 PPTX

最后一步是把标准化后的 SVG 页面导出为可编辑的 PowerPoint 文件。

常用命令：

```bash
python3 skills/ppt-master/scripts/svg_to_pptx.py <project_path>
```

如有特殊需求，也可以使用额外参数控制文本合并方式，例如：

```bash
python3 skills/ppt-master/scripts/svg_to_pptx.py <project_path> --merge-paragraphs
```

最终产物通常包括：

- `*.pptx`
- 最终 SVG 页面
- 讲稿备注文件
- 过程中使用的图像和模板资产

## 4. 角色分工

这个系统并不是单一步骤的“黑箱生成”，而是通过多角色协同完成：

- `Strategist`：负责信息理解、内容规划、受众定位、结构设计、风格确认
- `Image_Generator`：负责需要额外生成的配图与视觉资产
- `Executor`：负责逐页落地 SVG 页面并推进到导出

其中，`Executor` 阶段强调上下文连续性，因此不能把整套 SVG 页面外包给批量子任务或脚本生成器。

