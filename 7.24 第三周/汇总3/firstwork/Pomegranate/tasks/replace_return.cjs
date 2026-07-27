const fs = require('fs');
const lines = fs.readFileSync('src/pages/settings/index.tsx', 'utf8').split('\n');

const newReturn = `  return (
    <div className="settings-tabs-page">
      <div style={{ marginBottom: 16 }}>
        <Title level={3}>设置</Title>
      </div>

      <Tabs
        activeKey={activeTab}
        onChange={(key) => setActiveTab(key as TabKey)}
        tabPosition="left"
        destroyInactiveTabPane={false}
        className="settings-tabs-shell"
        items={[
          // ─── Tab 1: AI能力中心 ──────────────────────
          {
            key: TAB_KEYS.ai,
            label: <span className="flex items-center gap-2"><Bot size={16} />AI能力中心</span>,
            children: (
              <div className="settings-tabs-stack">
                {/* AI 模型配置 */}
                <Card
                  id="settings-ai-models"
                  title="AI 模型配置"
                  extra={
                    <Button type="primary" size="small" icon={<PlusOutlined />} onClick={openAddModel}>
                      添加模型
                    </Button>
                  }
                >
                  <Table
                    columns={modelColumns}
                    dataSource={models}
                    rowKey="id"
                    loading={modelsLoading}
                    pagination={false}
                    size="small"
                  />
                </Card>

                {/* 语音识别 */}
                <AsrSection />
              </div>
            ),
          },
          // ─── Tab 2: 应用管理 ────────────────────────
          {
            key: TAB_KEYS.apps,
            label: <span className="flex items-center gap-2"><AppWindow size={16} />应用管理</span>,
            children: (
              <div className="settings-tabs-stack">
                {/* MCP 服务器 */}
                <MCPServerSection />
              </div>
            ),
          },
          // ─── Tab 3: 应用配置 ──────────────────────────
          {
            key: TAB_KEYS.appConfig,
            label: <span className="flex items-center gap-2"><Cog size={16} />应用配置</span>,
            children: (
              <div className="settings-tabs-stack">
                <div id="settings-data-dir">
                  <DataDirSection />
                </div>

                <Card
                  id="settings-daily-md-path"
                  title={
                    <span className="flex items-center gap-2">
                      <FolderOpen size={16} />
                      日记文件存储路径
                    </span>
                  }
                >
                  <Alert
                    type="info"
                    showIcon
                    className="mb-3"
                    message="当前笔记仍按现有数据库方式保存"
                    description="这里仅保存一个用于日记 Markdown 文件输出的目录配置，不会改变当前笔记正文写入数据库的实现。"
                  />
                  <Typography.Paragraph type="secondary" style={{ marginBottom: 8, fontSize: 13 }}>
                    后续生成或同步日记 .md 文件时，可读取此路径作为保存目录。
                  </Typography.Paragraph>
                  <Space.Compact style={{ width: "100%" }}>
                    <Input
                      allowClear
                      value={dailyNoteMdPathDraft}
                      placeholder="请选择或输入日记 Markdown 文件保存目录"
                      onChange={(e) => setDailyNoteMdPathDraft(e.target.value)}
                    />
                    <Button icon={<FolderOpen size={14} />} onClick={handlePickDailyNoteMdPath}>
                      选择目录
                    </Button>
                    <Button
                      type="primary"
                      loading={dailyNoteMdPathSaving}
                      disabled={dailyNoteMdPathDraft.trim() === dailyNoteMdPath}
                      onClick={() => saveDailyNoteMdPath(dailyNoteMdPathDraft)}
                    >
                      保存
                    </Button>
                  </Space.Compact>
                  {dailyNoteMdPath && (
                    <div className="mt-2">
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        当前已保存：
                      </Text>
                      <Text code copyable style={{ fontSize: 12, wordBreak: "break-all" }}>
                        {dailyNoteMdPath}
                      </Text>
                    </div>
                  )}
                </Card>

                <Card
                  id="settings-orphan-assets"
                  size="small"
                  title={
                    <span className="flex items-center gap-2">
                      <Trash2 size={16} style={{ color: "var(--ant-color-primary)" }} />
                      维护 · 孤儿素材清理
                    </span>
                  }
                >
                  <OrphanAssetsPanel />
                </Card>

                <Card id="settings-auto-save" title="自动保存" size="small">
                  <Text type="secondary" style={{ fontSize: 13 }}>
                    编辑器和日记已内置自动保存机制：停止输入后 1.2 秒自动将内容写入数据库。
                    无需手动操作，关闭窗口或切换笔记时也会自动触发保存。
                  </Text>
                </Card>

                {/* 导入笔记 */}
                <Card
                  id="settings-import"
                  title={
                    <span className="flex items-center gap-2">
                      <FolderInput size={16} />
                      导入笔记
                    </span>
                  }
                >
                  <div className="mb-3">
                    <Typography.Paragraph type="secondary" style={{ marginBottom: 8, fontSize: 13 }}>
                      支持三种导入方式：从文件夹批量扫描 .md 文件；从本地选择 PDF 或 Word 文档。
                    </Typography.Paragraph>
                    <Space wrap>
                      <Select
                        placeholder="导入到文件夹（可选）"
                        allowClear
                        style={{ width: 200 }}
                        value={importFolderId}
                        onChange={setImportFolderId}
                        options={flattenFolders(folders)}
                      />
                      <Button
                        type="primary"
                        icon={<FolderInput size={14} />}
                        onClick={handleScanFolder}
                        loading={scanning || importing}
                      >
                        扫描 Markdown 文件夹
                      </Button>
                      <Button onClick={handleImportPdfs}>导入 PDF</Button>
                      <Button onClick={handleImportWord}>导入 Word</Button>
                    </Space>
                  </div>
                  {importing && importProgress && (
                    <div className="mb-3">
                      <Progress
                        percent={Math.round((importProgress.current / importProgress.total) * 100)}
                        size="small"
                        format={() => \`\${importProgress.current}/\${importProgress.total}\`}
                      />
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        正在导入: {importProgress.file_name}
                      </Text>
                    </div>
                  )}
                  {importResult && (
                    <Alert
                      type={importResult.errors.length > 0 ? "warning" : "success"}
                      showIcon
                      message={\`导入完成: \${importResult.imported} 篇成功, \${importResult.skipped} 篇跳过\`}
                      description={
                        importResult.errors.length > 0 ? (
                          <List
                            size="small"
                            dataSource={importResult.errors.slice(0, 10)}
                            renderItem={(err) => (
                              <List.Item style={{ padding: "2px 0", fontSize: 12 }}>
                                <Text type="danger">{err}</Text>
                              </List.Item>
                            )}
                            footer={
                              importResult.errors.length > 10 ? (
                                <Text type="secondary" style={{ fontSize: 11 }}>
                                  还有 {importResult.errors.length - 10} 条错误...
                                </Text>
                              ) : null
                            }
                          />
                        ) : undefined
                      }
                      closable
                      onClose={() => setImportResult(null)}
                    />
                  )}
                </Card>

                {/* 导出 Markdown */}
                <Card
                  id="settings-export"
                  title={
                    <span className="flex items-center gap-2">
                      <FolderOutput size={16} />
                      导出 Markdown
                    </span>
                  }
                >
                  <div className="mb-3">
                    <Typography.Paragraph type="secondary" style={{ marginBottom: 8, fontSize: 13 }}>
                      将笔记导出为 Markdown 文件，按文件夹结构组织。便于备份或迁移到其他笔记工具。
                    </Typography.Paragraph>
                    <Space>
                      <Select
                        placeholder="导出指定文件夹（可选，默认全部）"
                        allowClear
                        style={{ width: 240 }}
                        value={exportFolderId}
                        onChange={setExportFolderId}
                        options={flattenFolders(folders)}
                      />
                      <Button
                        type="primary"
                        icon={<FolderOutput size={14} />}
                        onClick={handleExport}
                        loading={exporting}
                      >
                        选择导出目录
                      </Button>
                    </Space>
                  </div>
                  {exporting && exportProgress && (
                    <div className="mb-3">
                      <Progress
                        percent={Math.round((exportProgress.current / exportProgress.total) * 100)}
                        size="small"
                        format={() => \`\${exportProgress.current}/\${exportProgress.total}\`}
                      />
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        正在导出: {exportProgress.file_name}
                      </Text>
                    </div>
                  )}
                  {exportResult && (
                    <Alert
                      type={exportResult.errors.length > 0 ? "warning" : "success"}
                      showIcon
                      message={\`导出完成: \${exportResult.exported} 篇笔记，附带 \${exportResult.assets_copied} 个资产文件\`}
                      description={
                        <div className="space-y-2">
                          <div style={{ fontSize: 12, fontFamily: "monospace", wordBreak: "break-all" }}>
                            {exportResult.root_dir}
                          </div>
                          <Space size="small">
                            <Button
                              size="small"
                              type="primary"
                              onClick={() => revealItemInDir(exportResult.root_dir).catch(() => {})}
                            >
                              打开所在文件夹
                            </Button>
                          </Space>
                          {exportResult.errors.length > 0 && (
                            <List
                              size="small"
                              dataSource={exportResult.errors.slice(0, 10)}
                              renderItem={(err) => (
                                <List.Item style={{ padding: "2px 0", fontSize: 12 }}>
                                  <Text type="danger">{err}</Text>
                                </List.Item>
                              )}
                            />
                          )}
                        </div>
                      }
                      closable
                      onClose={() => setExportResult(null)}
                    />
                  )}
                </Card>

                <div id="settings-sync">
                  <SyncTabs />
                </div>

                {/* 插件设置 Tab */}
                {pluginSettingsTabs.map((tab) => {
                  const sectionId = \`settings-plugin-\${tab.pluginId}:\${tab.id}\`;
                  const pluginName =
                    pluginManager.getPluginName(tab.pluginId) ?? tab.pluginId;
                  return (
                    <Card
                      key={sectionId}
                      id={sectionId}
                      size="small"
                      title={
                        <span className="flex items-center gap-2">
                          <span style={{ fontSize: 12, opacity: 0.6 }}>🔌</span>
                          {tab.title}
                          <span
                            style={{
                              fontSize: 11,
                              color: "var(--ant-color-text-quaternary)",
                              fontWeight: "normal",
                            }}
                          >
                            ({pluginName})
                          </span>
                        </span>
                      }
                    >
                      <PluginSettingsTabRenderer tab={tab} />
                    </Card>
                  );
                })}
              </div>
            ),
          },
          // ─── Tab 4: 个性设置 ──────────────────────────
          {
            key: TAB_KEYS.personal,
            label: <span className="flex items-center gap-2"><UserRound size={16} />个性设置</span>,
            children: (
              <div className="settings-tabs-stack">
                <div id="settings-task-reminder">
                  <Card title="待办提醒">
                    <div className="flex items-center justify-between py-1">
                      <div>
                        <div>任务默认提醒时刻</div>
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          新建任务时若未指定时间，自动填充此时刻；编辑/创建弹窗里的"默认"也指这个值
                        </Text>
                      </div>
                      <TimePicker
                        value={dayjs(allDayReminderTime, "HH:mm")}
                        onChange={handleAllDayReminderTimeChange}
                        format="HH:mm"
                        minuteStep={5}
                        allowClear={false}
                        style={{ width: 120 }}
                      />
                    </div>
                  </Card>
                </div>

                <Card
                  id="settings-templates"
                  title={
                    <span className="flex items-center gap-2">
                      <LayoutTemplate size={16} />
                      笔记模板
                    </span>
                  }
                  extra={
                    <Button
                      type="primary"
                      size="small"
                      icon={<PlusOutlined />}
                      onClick={openAddTemplate}
                    >
                      新建模板
                    </Button>
                  }
                >
                  <Table
                    columns={[
                      { title: "模板名称", dataIndex: "name", key: "name" },
                      {
                        title: "描述",
                        dataIndex: "description",
                        key: "description",
                        ellipsis: true,
                        render: (text: string) => (
                          <Text type="secondary" style={{ fontSize: 12 }}>
                            {text || "—"}
                          </Text>
                        ),
                      },
                      {
                        title: "操作",
                        key: "action",
                        width: 100,
                        render: (_: unknown, record: NoteTemplate) => (
                          <Space size="small">
                            <Button type="text" size="small" icon={<Pencil size={14} />} onClick={() => openEditTemplate(record)} />
                            <Popconfirm title="确认删除此模板？" onConfirm={() => handleDeleteTemplate(record.id)}>
                              <Button type="text" size="small" danger icon={<Trash2 size={14} />} />
                            </Popconfirm>
                          </Space>
                        ),
                      },
                    ]}
                    dataSource={tplList}
                    rowKey="id"
                    loading={tplLoading}
                    pagination={false}
                    size="small"
                  />
                </Card>

                <div id="settings-hidden-pin">
                  <HiddenPinSection />
                </div>

                <div id="settings-shortcuts">
                  <ShortcutsSection />
                </div>
              </div>
            ),
          },
          // ─── Tab 5: 显示设置 ──────────────────────────
          {
            key: TAB_KEYS.display,
            label: <span className="flex items-center gap-2"><Monitor size={16} />显示设置</span>,
            children: (
              <div className="settings-tabs-stack">
                <AppearanceSection />

                <Card
                  id="settings-editor"
                  title={
                    <span className="flex items-center gap-2">
                      <Type size={16} />
                      编辑器外观
                    </span>
                  }
                >
                  <div className="flex items-center justify-between py-1">
                    <div>
                      <div>正文字体</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        用户系统未装首选字体时自动 fallback 到下一项，不会出错
                      </Text>
                    </div>
                    <Select
                      value={editorFontFamily}
                      onChange={(v) => setEditorFontFamily(v as EditorFontFamily)}
                      style={{ width: 220 }}
                      options={(Object.keys(EDITOR_FONT_LABELS) as EditorFontFamily[]).map(
                        (key) => ({
                          value: key,
                          label: (
                            <span style={{ fontFamily: EDITOR_FONT_STACKS[key] || undefined }}>
                              {EDITOR_FONT_LABELS[key]}
                            </span>
                          ),
                        }),
                      )}
                    />
                  </div>
                  <div
                    className="flex items-center justify-between py-1 mt-2"
                    style={{ borderTop: "1px solid #f0f0f0", paddingTop: 12 }}
                  >
                    <div>
                      <div>正文字号</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        标题、代码块按比例缩放
                      </Text>
                    </div>
                    <Select
                      value={editorFontSize}
                      onChange={setEditorFontSize}
                      style={{ width: 120 }}
                      options={EDITOR_FONT_SIZE_OPTIONS.map((s) => ({
                        value: s,
                        label: \`\${s} px\`,
                      }))}
                    />
                  </div>
                  <div
                    className="flex items-center justify-between py-1 mt-2"
                    style={{ borderTop: "1px solid #f0f0f0", paddingTop: 12 }}
                  >
                    <div>
                      <div>行距</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        段落行间距倍数
                      </Text>
                    </div>
                    <Select
                      value={editorLineHeight}
                      onChange={setEditorLineHeight}
                      style={{ width: 120 }}
                      options={EDITOR_LINE_HEIGHT_OPTIONS.map((h) => ({
                        value: h,
                        label: h.toFixed(1),
                      }))}
                    />
                  </div>

                  {/* 阅读列宽 */}
                  <div
                    className="flex items-center justify-between py-1"
                    style={{ borderTop: "1px solid #f0f0f0", paddingTop: 12, paddingBottom: 12 }}
                  >
                    <div>
                      <div>阅读列宽</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        限制正文宽度并居中，宽屏下一行不再横扫一大片
                      </Text>
                    </div>
                    <Select
                      value={editorReadingWidth}
                      onChange={setEditorReadingWidth}
                      style={{ width: 160 }}
                      options={EDITOR_READING_WIDTH_OPTIONS.map((w) => ({
                        value: w,
                        label: EDITOR_READING_WIDTH_LABELS[w] ?? \`\${w} px\`,
                      }))}
                    />
                  </div>

                  {/* 纸张观感 */}
                  <div
                    className="flex items-center justify-between py-1"
                    style={{ borderTop: "1px solid #f0f0f0", paddingTop: 12, paddingBottom: 12 }}
                  >
                    <div>
                      <div>纸张观感</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        正文区变白卡 + 阴影，外层垫浅灰底，像在纸上写
                      </Text>
                    </div>
                    <Switch checked={editorPaper} onChange={setEditorPaper} />
                  </div>

                  {/* 背景纹理 */}
                  <div
                    className="flex items-center justify-between py-1"
                    style={{ borderTop: "1px solid #f0f0f0", paddingTop: 12, paddingBottom: 12 }}
                  >
                    <div>
                      <div>背景纹理</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        横线 / 网格底纹，还原笔记本质感
                      </Text>
                    </div>
                    <Radio.Group
                      optionType="button"
                      buttonStyle="solid"
                      size="small"
                      value={editorRuleLines}
                      onChange={(e) => setEditorRuleLines(e.target.value as EditorRuleLines)}
                      options={(Object.keys(EDITOR_RULE_LABELS) as EditorRuleLines[]).map(
                        (k) => ({ value: k, label: EDITOR_RULE_LABELS[k] }),
                      )}
                    />
                  </div>

                  {/* 首行缩进 */}
                  <div
                    className="flex items-center justify-between py-1"
                    style={{ borderTop: "1px solid #f0f0f0", paddingTop: 12, paddingBottom: 12 }}
                  >
                    <div>
                      <div>首行缩进</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        顶层段落首行缩进 2 字符（中文写作习惯）
                      </Text>
                    </div>
                    <Switch
                      checked={editorFirstLineIndent}
                      onChange={setEditorFirstLineIndent}
                    />
                  </div>

                  {/* 高亮快捷键 */}
                  <div
                    className="flex items-center justify-between py-1"
                    style={{ borderTop: "1px solid #f0f0f0", paddingTop: 12, paddingBottom: 4 }}
                  >
                    <div>
                      <div>高亮快捷键</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        选中文字后按快捷键加/取消高亮（后续版本支持自定义键位）
                      </Text>
                    </div>
                    <Tooltip title="当前默认快捷键 Ctrl+Shift+H">
                      <Tag color="blue">Ctrl+Shift+H</Tag>
                    </Tooltip>
                  </div>

                  <div style={{ borderTop: "1px solid #f0f0f0", marginTop: 12, paddingTop: 12 }}>
                    <Text type="secondary" style={{ fontSize: 12, display: "block", marginBottom: 6 }}>
                      预览
                    </Text>
                    <div
                      style={{
                        padding: "12px 14px",
                        background: "var(--ant-color-fill-quaternary, #fafafa)",
                        border: "1px solid var(--ant-color-border-secondary, #f0f0f0)",
                        borderRadius: 6,
                        fontFamily: EDITOR_FONT_STACKS[editorFontFamily] || undefined,
                        fontSize: editorFontSize,
                        lineHeight: editorLineHeight,
                      }}
                    >
                      春有百花秋有月，夏有凉风冬有雪。
                      <br />
                      The quick brown fox jumps over the lazy dog. 1234567890
                    </div>
                    <div className="flex justify-end mt-3">
                      <Button size="small" onClick={resetEditorTypography}>
                        恢复默认
                      </Button>
                    </div>
                  </div>
                </Card>
              </div>
            ),
          },
          // ─── Tab 6: 更新启动 ──────────────────────────
          {
            key: TAB_KEYS.startup,
            label: <span className="flex items-center gap-2"><Rocket size={16} />更新启动</span>,
            children: (
              <div className="settings-tabs-stack">
                <Card id="settings-update" title="软件更新">
                  <Space wrap>
                    <Button
                      icon={<SyncOutlined spin={checking} />}
                      onClick={handleCheckUpdate}
                      loading={checking}
                    >
                      检查更新
                    </Button>
                    <Text type="secondary">当前版本: {appVersion}</Text>
                    <Button
                      type="link"
                      size="small"
                      onClick={() => openUrl("https://me.bit.edu.cn/")}
                    >
                      官网 https://me.bit.edu.cn/
                    </Button>
                  </Space>
                </Card>

                <Card
                  id="settings-startup"
                  title={
                    <span className="flex items-center gap-2">
                      <Power size={16} />
                      启动设置
                    </span>
                  }
                >
                  <div className="flex items-center justify-between py-1">
                    <div>
                      <div>开机自动启动</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        登录系统后自动打开知识库，用于定时提醒等后台任务
                      </Text>
                    </div>
                    <Switch
                      checked={autostartEnabled}
                      loading={autostartLoading}
                      onChange={handleAutostartToggle}
                    />
                  </div>
                  <div
                    className="flex items-center justify-between py-1 mt-2"
                    style={{ borderTop: "1px solid #f0f0f0", paddingTop: 12 }}
                  >
                    <div>
                      <div>启动时最小化到托盘</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        仅在开机自动启动时生效；手动双击打开仍会正常显示窗口
                      </Text>
                    </div>
                    <Switch
                      checked={startMinimized}
                      loading={startMinimizedLoading}
                      disabled={!autostartEnabled}
                      onChange={handleStartMinimizedToggle}
                    />
                  </div>
                  <div
                    className="flex items-center justify-between py-1 mt-2"
                    style={{ borderTop: "1px solid #f0f0f0", paddingTop: 12 }}
                  >
                    <div>
                      <div>允许多开实例</div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        关闭时再次启动会唤起已有窗口（默认）；开启后每次启动都开新窗口，
                        注意多个实例同时写同一份数据库可能导致冲突。下次启动生效。
                      </Text>
                    </div>
                    <Switch
                      checked={multiInstanceEnabled}
                      loading={multiInstanceLoading}
                      onChange={handleMultiInstanceToggle}
                    />
                  </div>
                  <div
                    className="py-1 mt-2"
                    style={{ borderTop: "1px solid #f0f0f0", paddingTop: 12 }}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div style={{ flex: 1 }}>
                        <div>关闭窗口时</div>
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          点击右上角关闭按钮的行为。选"每次询问"会弹出三选一对话框。
                        </Text>
                      </div>
                      <Radio.Group
                        value={closeAction}
                        onChange={(e) => handleCloseActionChange(e.target.value)}
                        optionType="button"
                        buttonStyle="solid"
                      >
                        <Radio.Button value="ask">每次询问</Radio.Button>
                        <Radio.Button value="minimize">最小化到托盘</Radio.Button>
                        <Radio.Button value="exit">直接退出</Radio.Button>
                      </Radio.Group>
                    </div>
                  </div>
                </Card>

                <FeatureModulesSection title="侧边栏" />
              </div>
            ),
          },
          // ─── Tab 7: 关于 ─────────────────────────────
          {
            key: TAB_KEYS.about,
            label: <span className="flex items-center gap-2"><Info size={16} />关于</span>,
            children: (
              <div className="settings-tabs-stack">
                <AboutContent
                  showHeader={false}
                  showSponsor={false}
                  showRecommend={false}
                  idPrefix="settings-about"
                />
              </div>
            ),
          },
        ]}
      />

      {/* 导入预览弹窗 */}
      <Modal
        title={\`选择要导入的文件（共 \${scannedFiles.length} 个）\`}
        open={scanModalOpen}
        onCancel={() => setScanModalOpen(false)}
        onOk={handleConfirmImport}
        okText={\`导入 \${selectedPaths.size} 个文件\`}
        cancelText="取消"
        width={600}
        styles={{ body: { maxHeight: 400, overflow: "auto" } }}
      >
        <div className="flex items-center justify-between mb-3 pb-2" style={{ borderBottom: "1px solid #f0f0f0" }}>
          <Checkbox
            checked={selectedPaths.size === scannedFiles.length && scannedFiles.length > 0}
            indeterminate={selectedPaths.size > 0 && selectedPaths.size < scannedFiles.length}
            onChange={toggleSelectAll}
          >
            全选 / 取消全选
          </Checkbox>
          <Text type="secondary" style={{ fontSize: 12 }}>
            已选 {selectedPaths.size} / {scannedFiles.length}
          </Text>
        </div>
        <div className="mb-3 pb-2" style={{ borderBottom: "1px solid #f0f0f0" }}>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1 mb-2" style={{ fontSize: 12 }}>
            <span>🆕 全新 <strong>{matchStats.news}</strong></span>
            <span title="路径已匹配到已有笔记（上次导入过）">🔁 已导入过 <strong>{matchStats.paths}</strong></span>
            <span title="路径不同但标题+内容与已有笔记一致，可能是用户搬动过文件">⚠️ 可能重复 <strong>{matchStats.fuzzies}</strong></span>
          </div>
          {matchStats.conflicts > 0 && (
            <div>
              <Text style={{ fontSize: 12 }}>遇到已存在的文件：</Text>
              <Radio.Group
                value={conflictPolicy}
                onChange={(e) => setConflictPolicy(e.target.value as ImportConflictPolicy)}
                size="small"
                style={{ marginLeft: 8 }}
              >
                <Radio value="skip">跳过（推荐）</Radio>
                <Radio value="duplicate">创建副本</Radio>
              </Radio.Group>
            </div>
          )}
        </div>
        <div className="mb-3 pb-2" style={{ borderBottom: "1px solid #f0f0f0" }}>
          <Checkbox checked={preserveRoot} onChange={(e) => setPreserveRoot(e.target.checked)}>
            <span style={{ fontSize: 13 }}>保留源文件夹作为根</span>
          </Checkbox>
          <div className="mt-1" style={{ paddingLeft: 24 }}>
            <Text type="secondary" style={{ fontSize: 11 }}>
              导入时按源目录层级自动创建子文件夹，同名文件夹复用已有记录。
              {preserveRoot ? "将在目标下创建与源目录同名的根文件夹。" : "子目录直接挂到目标位置。"}
            </Text>
          </div>
        </div>
        <List
          size="small"
          dataSource={scannedFiles}
          renderItem={(file) => (
            <List.Item style={{ padding: "6px 0" }}>
              <Checkbox
                checked={selectedPaths.has(file.path)}
                onChange={() => toggleFileSelection(file.path)}
                style={{ marginRight: 8 }}
              />
              <div className="flex-1 min-w-0">
                <Text ellipsis style={{ fontSize: 13 }}>{file.name}.md</Text>
                {file.relative_dir && (
                  <div>
                    <Text type="secondary" ellipsis style={{ fontSize: 11, display: "block" }}>
                      {file.relative_dir}
                    </Text>
                  </div>
                )}
              </div>
              <Text type="secondary" style={{ fontSize: 11, flexShrink: 0 }}>
                {file.size < 1024
                  ? \`\${file.size} B\`
                  : file.size < 1048576
                    ? \`\${(file.size / 1024).toFixed(1)} KB\`
                    : \`\${(file.size / 1048576).toFixed(1)} MB\`}
              </Text>
            </List.Item>
          )}
        />
      </Modal>

      {/* 添加/编辑模型弹窗 */}
      <Modal
        title={editingModel ? "编辑 AI 模型" : "添加 AI 模型"}
        open={modelModalOpen}
        onCancel={() => setModelModalOpen(false)}
        destroyOnHidden
        className="ai-model-modal"
        styles={{ body: { maxHeight: "calc(100vh - 220px)", overflowY: "auto" } }}
        footer={
          <Space>
            <Button
              icon={<Zap size={14} />}
              loading={testingModelId === -1}
              disabled={testingModelId !== null && testingModelId !== -1}
              onClick={handleTestForm}
            >
              测试连接
            </Button>
            <Button onClick={() => setModelModalOpen(false)}>取消</Button>
            <Button type="primary" onClick={handleModelSave}>
              保存
            </Button>
          </Space>
        }
      >
        <Form form={form} layout="vertical" initialValues={{ provider: "ollama", api_url: DEFAULT_URLS.ollama }}>
          <Form.Item name="name" label="名称" rules={[{ required: true, message: "请输入模型名称" }]}>
            <Input placeholder="如: GPT-4o Mini" />
          </Form.Item>
          <Form.Item
            name="provider" label="提供商"
            extra="除 Ollama 外都按 OpenAI 兼容协议处理。选「自定义」可填任意 baseUrl（OpenRouter / Moonshot / 字节豆包 / 自建网关 / lm studio 等任何 OpenAI 兼容服务）"
            rules={[{ required: true }]}
          >
            <Select options={PROVIDERS} onChange={handleProviderChange} />
          </Form.Item>
          <Form.Item name="api_url" label="API 地址" extra="支持任意 OpenAI 兼容服务的 base_url（不含 /chat/completions 后缀）" rules={[{ required: true, message: "请输入 API 地址" }]}>
            <Input placeholder={DEFAULT_URLS[watchedProvider] || "https://api.openai.com/v1"} />
          </Form.Item>
          <Form.Item name="api_key" label="API Key">
            <Input.Password placeholder="sk-... (Ollama 无需填写)" />
          </Form.Item>
          <Form.Item
            name="model_id" label="模型标识"
            extra="✏️ 可直接输入任意模型名（如 anthropic/claude-sonnet-4.6、moonshotai/kimi-k2 等），不必限于下拉候选"
            rules={[{ required: true, message: "请输入或选择模型标识" }]}
          >
            <AutoComplete
              options={MODEL_PRESETS[watchedProvider] || []}
              placeholder={MODEL_ID_PLACEHOLDERS[watchedProvider] || "如: gpt-4o-mini / qwen2.5:7b"}
              filterOption={(input, option) => (option?.value as string)?.toLowerCase().includes(input.toLowerCase())}
              allowClear
            />
          </Form.Item>
          <Form.Item name="max_context" label="最大上下文 token" extra="从下拉选常用量级，或手动输入任意数字。不确定就保留 32000。" initialValue={32000}>
            <AutoComplete
              placeholder="32000"
              options={[
                { value: 32000, label: "32K  （OpenAI 老款 / 默认）" },
                { value: 64000, label: "64K" },
                { value: 128000, label: "128K （DeepSeek / GPT-4o / 智谱）" },
                { value: 200000, label: "200K （Claude）" },
                { value: 1000000, label: "1M   （GLM-Long / MiniMax-M1）" },
                { value: 2000000, label: "2M" },
              ]}
              filterOption={(input, option) => {
                const q = input.trim().toLowerCase();
                if (!q) return true;
                return String(option?.value ?? "").includes(q) || String(option?.label ?? "").toLowerCase().includes(q);
              }}
              style={{ width: "100%" }}
            />
          </Form.Item>
        </Form>
      </Modal>

      {/* 模板编辑弹窗 */}
      <Modal
        title={editingTpl ? "编辑模板" : "新建模板"}
        open={tplModalOpen}
        onOk={handleTemplateSave}
        onCancel={() => setTplModalOpen(false)}
        okText="保存"
        cancelText="取消"
        destroyOnHidden
        width={820}
        styles={{ body: { maxHeight: "calc(100vh - 240px)", overflow: "auto" } }}
      >
        <Form form={tplForm} layout="vertical" className="mt-4">
          <Form.Item name="name" label="模板名称" rules={[{ required: true, message: "请输入模板名称" }]}>
            <Input placeholder="如：会议记录" />
          </Form.Item>
          <Form.Item name="description" label="描述" initialValue="">
            <Input placeholder="简要描述模板用途" />
          </Form.Item>
          <Form.Item
            name="content"
            label={
              <div className="flex items-center justify-between w-full">
                <span>模板内容</span>
                <span style={{ fontSize: 12, color: "var(--ant-color-text-tertiary)", fontWeight: "normal" }}>
                  可用变量：{"{{date}} / {{time}} / {{datetime}} / {{weekday}} / {{year}} / {{month}} / {{day}} / {{title}}"}
                </span>
              </div>
            }
            initialValue=""
            valuePropName="content"
          >
            <TiptapEditor content="" onChange={() => {}} placeholder="输入模板内容（支持富文本），创建笔记时将自动填充" />
          </Form.Item>
        </Form>
      </Modal>

      <UpdateModal
        open={updateModalOpen}
        onClose={() => setUpdateModalOpen(false)}
        update={update}
      />
    </div>
  );
`;

// Find line indices
let startIdx = -1, endIdx = -1;
for (let i = 0; i < lines.length; i++) {
  if (lines[i].trim() === 'return (' && i > 1000) { startIdx = i; break; }
}
for (let i = lines.length - 1; i >= 0; i--) {
  if (lines[i].includes('import { useIsMobile }')) {
    for (let j = i - 1; j >= 0; j--) {
      if (lines[j].trim() === '}') { endIdx = j; break; }
    }
    break;
  }
}

if (startIdx === -1 || endIdx === -1) {
  console.error('Could not find boundaries');
  process.exit(1);
}

// Replace
const before = lines.slice(0, startIdx);
const after = lines.slice(endIdx + 1);
const newLines = newReturn.split('\n');
const result = [...before, ...newLines, ...after];
fs.writeFileSync('src/pages/settings/index.tsx', result.join('\n'), 'utf8');
console.log('Done. Replaced lines', startIdx + 1, 'to', endIdx + 1, '(' + (endIdx - startIdx) + ' lines) with', newLines.length, 'lines');
console.log('Total file lines:', result.length);
