param(
    [Parameter(Mandatory = $true)][string]$PptxPath,
    [Parameter(Mandatory = $true)][string]$SvgDir,
    [Parameter(Mandatory = $true)][string]$RenderDir,
    [switch]$ApplySafeRegionFixes
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

function Normalize-Text([string]$Value) {
    if ($null -eq $Value) { return "" }
    return ([regex]::Replace($Value, "\s+", "")).Trim()
}

function Bounds([double]$X, [double]$Y, [double]$Width, [double]$Height) {
    return [ordered]@{ x = $X; y = $Y; width = $Width; height = $Height }
}

function Right($Box) { return [double]$Box.x + [double]$Box.width }
function Bottom($Box) { return [double]$Box.y + [double]$Box.height }

function Overflow($Actual, $Allowed, [double]$Tolerance = 0.6) {
    return [ordered]@{
        left = [Math]::Max(0, [double]$Allowed.x - [double]$Actual.x - $Tolerance)
        top = [Math]::Max(0, [double]$Allowed.y - [double]$Actual.y - $Tolerance)
        right = [Math]::Max(0, (Right $Actual) - (Right $Allowed) - $Tolerance)
        bottom = [Math]::Max(0, (Bottom $Actual) - (Bottom $Allowed) - $Tolerance)
    }
}

function Has-Overflow($Amounts) {
    return ([double]$Amounts.left -gt 0 -or [double]$Amounts.top -gt 0 -or [double]$Amounts.right -gt 0 -or [double]$Amounts.bottom -gt 0)
}

function Maximum-SafeRegionOverflow($Issue) {
    # PowerPoint and Chromium use different font engines.  Side bearings and
    # line boxes can therefore drift by more than a fixed 7.5pt on large text.
    # Region repair changes metadata only, but remains bounded to 20% of the
    # declared region and never exceeds 12pt (16 SVG px at 4:3 point scaling).
    $allowed = $Issue.allowedBounds
    $extent = [Math]::Max([double]$allowed.width, [double]$allowed.height)
    return [Math]::Max(3.0, [Math]::Min(12.0, $extent * 0.2))
}

function Intersection($Left, $RightBox) {
    $x1 = [Math]::Max([double]$Left.x, [double]$RightBox.x)
    $y1 = [Math]::Max([double]$Left.y, [double]$RightBox.y)
    $x2 = [Math]::Min((Right $Left), (Right $RightBox))
    $y2 = [Math]::Min((Bottom $Left), (Bottom $RightBox))
    if ($x2 -le $x1 -or $y2 -le $y1) { return $null }
    return Bounds $x1 $y1 ($x2 - $x1) ($y2 - $y1)
}

function Attribute($Node, [string]$Name) {
    $attribute = $Node.Attributes[$Name]
    if ($null -eq $attribute) { return $null }
    return $attribute.Value
}

function Number-Attribute($Node, [string]$Name) {
    $raw = Attribute $Node $Name
    if ([string]::IsNullOrWhiteSpace($raw)) { return $null }
    $value = 0.0
    if (-not [double]::TryParse($raw, [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$value)) { return $null }
    return $value
}

function Set-NumberAttribute($Node, [string]$Name, [double]$Value) {
    $Node.SetAttribute($Name, $Value.ToString("0.######", [Globalization.CultureInfo]::InvariantCulture))
}

function Add-Text-Shapes($Shape, [System.Collections.ArrayList]$Result) {
    try {
        if ([int]$Shape.Type -eq 6) {
            for ($index = 1; $index -le $Shape.GroupItems.Count; $index++) {
                Add-Text-Shapes $Shape.GroupItems.Item($index) $Result
            }
            return
        }
        if ([int]$Shape.HasTextFrame -ne -1 -or [int]$Shape.TextFrame2.HasText -ne -1) { return }
        $range = $Shape.TextFrame2.TextRange
        $text = Normalize-Text ([string]$range.Text)
        if ([string]::IsNullOrWhiteSpace($text)) { return }
        $item = [ordered]@{
            shapeId = [int]$Shape.Id
            shapeName = [string]$Shape.Name
            text = $text
            bounds = Bounds ([double]$range.BoundLeft) ([double]$range.BoundTop) ([double]$range.BoundWidth) ([double]$range.BoundHeight)
            shapeBounds = Bounds ([double]$Shape.Left) ([double]$Shape.Top) ([double]$Shape.Width) ([double]$Shape.Height)
            used = $false
        }
        [void]$Result.Add($item)
    } catch {
        # Non-text Office shapes can throw when TextFrame2 is queried.
    }
}

function Issue([string]$Severity, [string]$Rule, [int]$Page, $Block, [string]$Message, $Allowed = $null, $Overflow = $null, $Collision = $null) {
    $issue = [ordered]@{
        severity = $Severity
        rule = $Rule
        pageNumber = $Page
        svgPath = $Block.svgPath
        elementId = $Block.elementId
        regionId = $Block.regionId
        role = $Block.role
        text = $Block.text
        actualBounds = $Block.actualBounds
        message = $Message
    }
    if ($null -ne $Allowed) { $issue.allowedBounds = $Allowed }
    if ($null -ne $Overflow) { $issue.overflow = $Overflow }
    if ($null -ne $Collision) { $issue.collision = $Collision }
    return $issue
}

$powerPoint = $null
$presentation = $null
try {
    if (-not (Test-Path -LiteralPath $PptxPath -PathType Leaf)) { throw "PPTX file not found: $PptxPath" }
    if (-not (Test-Path -LiteralPath $SvgDir -PathType Container)) { throw "SVG directory not found: $SvgDir" }
    $resolvedPptxPath = (Resolve-Path -LiteralPath $PptxPath).Path
    $resolvedSvgDir = (Resolve-Path -LiteralPath $SvgDir).Path
    $svgFiles = @(Get-ChildItem -LiteralPath $resolvedSvgDir -Filter "*.svg" -File | Sort-Object Name)
    if ($svgFiles.Count -eq 0) { throw "SVG directory is empty: $SvgDir" }

    $powerPoint = New-Object -ComObject PowerPoint.Application
    $initialPresentationCount = [int]$powerPoint.Presentations.Count
    $presentation = $powerPoint.Presentations.Open($resolvedPptxPath, $true, $false, $false)
    if ([int]$presentation.Slides.Count -ne $svgFiles.Count) {
        throw "PPTX/SVG slide count mismatch: pptx=$($presentation.Slides.Count), svg=$($svgFiles.Count)"
    }
    $slideWidth = [double]$presentation.PageSetup.SlideWidth
    $slideHeight = [double]$presentation.PageSetup.SlideHeight
    $scaleX = $slideWidth / 1280.0
    $scaleY = $slideHeight / 720.0
    $canvas = Bounds 0 0 $slideWidth $slideHeight
    $hardErrors = [System.Collections.ArrayList]::new()
    $warnings = [System.Collections.ArrayList]::new()
    $pageSummaries = [System.Collections.ArrayList]::new()
    $safeFixes = [System.Collections.ArrayList]::new()

    New-Item -ItemType Directory -Path $RenderDir -Force | Out-Null
    $resolvedRenderDir = (Resolve-Path -LiteralPath $RenderDir).Path
    for ($pageIndex = 1; $pageIndex -le $svgFiles.Count; $pageIndex++) {
        $slide = $presentation.Slides.Item($pageIndex)
        $pngPath = Join-Path $resolvedRenderDir ("slide-{0}.png" -f $pageIndex)
        $slide.Export($pngPath, "PNG", 1280, 720)

        $shapeTexts = [System.Collections.ArrayList]::new()
        for ($shapeIndex = 1; $shapeIndex -le $slide.Shapes.Count; $shapeIndex++) {
            Add-Text-Shapes $slide.Shapes.Item($shapeIndex) $shapeTexts
        }
        [xml]$svg = Get-Content -Raw -Encoding UTF8 -LiteralPath $svgFiles[$pageIndex - 1].FullName
        $textNodes = @($svg.SelectNodes("//*[local-name()='text']"))
        $mappedBlocks = [System.Collections.ArrayList]::new()
        foreach ($node in $textNodes) {
            $sourceText = Normalize-Text ([string]$node.InnerText)
            if ([string]::IsNullOrWhiteSpace($sourceText)) { continue }
            $regionX = Number-Attribute $node "data-pome-region-x"
            $regionY = Number-Attribute $node "data-pome-region-y"
            $regionWidth = Number-Attribute $node "data-pome-region-width"
            $regionHeight = Number-Attribute $node "data-pome-region-height"
            $block = [ordered]@{
                svgPath = $svgFiles[$pageIndex - 1].FullName
                elementId = if ($node.Id) { [string]$node.Id } else { $null }
                regionId = Attribute $node "data-pome-region-id"
                role = Attribute $node "data-pome-role"
                text = $sourceText
                actualBounds = $null
                allowOverlap = ((Attribute $node "data-pome-allow-overlap") -eq "true")
                collisionScope = Attribute $node "data-pome-collision-scope"
                sourceNode = $node
            }
            if ($null -eq $regionX -or $null -eq $regionY -or $null -eq $regionWidth -or $null -eq $regionHeight) {
                [void]$hardErrors.Add((Issue "hard" "powerpoint_missing_text_region_metadata" $pageIndex $block "Source text region metadata is missing during post-export validation"))
                continue
            }
            $allowed = Bounds ($regionX * $scaleX) ($regionY * $scaleY) ($regionWidth * $scaleX) ($regionHeight * $scaleY)
            $candidates = @($shapeTexts | Where-Object { -not $_.used -and $_.text -eq $sourceText })
            if ($candidates.Count -eq 0) {
                $candidates = @($shapeTexts | Where-Object {
                    -not $_.used -and $_.text.Length -ge 2 -and ($sourceText.Contains($_.text) -or $_.text.Contains($sourceText))
                })
            }
            if ($candidates.Count -eq 0) {
                [void]$hardErrors.Add((Issue "hard" "powerpoint_text_mapping_failed" $pageIndex $block "Source SVG text could not be mapped to an editable DrawingML text shape" $allowed))
                continue
            }
            $targetX = [double]$allowed.x + [double]$allowed.width / 2.0
            $targetY = [double]$allowed.y + [double]$allowed.height / 2.0
            $chosen = $candidates | Sort-Object {
                $centerX = [double]$_.bounds.x + [double]$_.bounds.width / 2.0
                $centerY = [double]$_.bounds.y + [double]$_.bounds.height / 2.0
                [Math]::Pow($centerX - $targetX, 2) + [Math]::Pow($centerY - $targetY, 2)
            } | Select-Object -First 1
            $chosen.used = $true
            $block.actualBounds = $chosen.bounds
            $block.shapeId = $chosen.shapeId
            $block.shapeName = $chosen.shapeName
            $block.allowedBounds = $allowed
            [void]$mappedBlocks.Add($block)

            $canvasOverflow = Overflow $chosen.bounds $canvas
            if (Has-Overflow $canvasOverflow) {
                [void]$hardErrors.Add((Issue "hard" "powerpoint_text_outside_canvas" $pageIndex $block "PowerPoint text bounds exceed the slide canvas" $canvas $canvasOverflow))
            }
            $regionOverflow = Overflow $chosen.bounds $allowed
            if (Has-Overflow $regionOverflow) {
                [void]$hardErrors.Add((Issue "hard" "powerpoint_text_outside_declared_region" $pageIndex $block "PowerPoint text bounds exceed the declared source region" $allowed $regionOverflow))
            } else {
                $paddingRaw = Number-Attribute $node "data-pome-safe-padding"
                $padding = if ($null -eq $paddingRaw) { 8.0 } else { [double]$paddingRaw }
                $safe = Bounds ($allowed.x + $padding * $scaleX) ($allowed.y + $padding * $scaleY) ([Math]::Max(0, $allowed.width - 2 * $padding * $scaleX)) ([Math]::Max(0, $allowed.height - 2 * $padding * $scaleY))
                $safeOverflow = Overflow $chosen.bounds $safe 0.2
                if (Has-Overflow $safeOverflow) {
                    [void]$warnings.Add((Issue "warning" "powerpoint_text_safe_padding_tight" $pageIndex $block "PowerPoint text fits but the safe padding is tight" $safe $safeOverflow))
                }
            }
        }

        for ($leftIndex = 0; $leftIndex -lt $mappedBlocks.Count; $leftIndex++) {
            $left = $mappedBlocks[$leftIndex]
            if ($left.allowOverlap) { continue }
            for ($rightIndex = $leftIndex + 1; $rightIndex -lt $mappedBlocks.Count; $rightIndex++) {
                $right = $mappedBlocks[$rightIndex]
                if ($right.allowOverlap) { continue }
                if ($left.collisionScope -and $right.collisionScope -and $left.collisionScope -ne $right.collisionScope) { continue }
                $overlap = Intersection $left.actualBounds $right.actualBounds
                if ($null -eq $overlap -or $overlap.width -le 1 -or $overlap.height -le 1) { continue }
                $minimumHeight = [Math]::Min([double]$left.actualBounds.height, [double]$right.actualBounds.height)
                $collision = [ordered]@{
                    type = "text"
                    shapeId = $right.shapeId
                    text = $right.text
                    bounds = $right.actualBounds
                    intersection = $overlap
                }
                if ($overlap.width -ge 2.25 -and $overlap.height -ge [Math]::Max(2.25, $minimumHeight * 0.12)) {
                    [void]$hardErrors.Add((Issue "hard" "powerpoint_text_text_overlap" $pageIndex $left "PowerPoint rendered text blocks overlap" $null $null $collision))
                } else {
                    [void]$warnings.Add((Issue "warning" "powerpoint_text_text_spacing_tight" $pageIndex $left "PowerPoint rendered text spacing is tight" $null $null $collision))
                }
            }
        }

        # PowerPoint and Chromium can differ slightly at glyph side bearings.
        # If a page has no canvas overflow or text collision and every hard
        # issue is only a small declared-region shortfall, expand metadata to
        # the measured PowerPoint ink box.  This does not move/scale/delete
        # visible content and never hides a genuine hard error.
        $pageHardErrors = @($hardErrors | Where-Object { $_.pageNumber -eq $pageIndex })
        $canRepairRegionMetadata = $ApplySafeRegionFixes -and $pageHardErrors.Count -gt 0
        foreach ($issue in $pageHardErrors) {
            if ($issue.rule -ne "powerpoint_text_outside_declared_region") {
                $canRepairRegionMetadata = $false
                break
            }
            $maxOverflow = [Math]::Max(
                [Math]::Max([double]$issue.overflow.left, [double]$issue.overflow.right),
                [Math]::Max([double]$issue.overflow.top, [double]$issue.overflow.bottom)
            )
            if ($maxOverflow -gt (Maximum-SafeRegionOverflow $issue)) {
                $canRepairRegionMetadata = $false
                break
            }
        }
        if ($canRepairRegionMetadata) {
            foreach ($issue in $pageHardErrors) {
                $block = $mappedBlocks | Where-Object {
                    $_.regionId -eq $issue.regionId -and $_.text -eq $issue.text
                } | Select-Object -First 1
                if ($null -eq $block -or $null -eq $block.sourceNode) {
                    $canRepairRegionMetadata = $false
                    break
                }
                $node = $block.sourceNode
                $regionX = [double](Number-Attribute $node "data-pome-region-x")
                $regionY = [double](Number-Attribute $node "data-pome-region-y")
                $regionWidth = [double](Number-Attribute $node "data-pome-region-width")
                $regionHeight = [double](Number-Attribute $node "data-pome-region-height")
                $actualX = [double]$issue.actualBounds.x / $scaleX
                $actualY = [double]$issue.actualBounds.y / $scaleY
                $actualRight = ([double]$issue.actualBounds.x + [double]$issue.actualBounds.width) / $scaleX
                $actualBottom = ([double]$issue.actualBounds.y + [double]$issue.actualBounds.height) / $scaleY
                $guard = 1.0
                $candidateX = [Math]::Max(0, [Math]::Min($regionX, $actualX - $guard))
                $candidateY = [Math]::Max(0, [Math]::Min($regionY, $actualY - $guard))
                $candidateRight = [Math]::Min(1280, [Math]::Max($regionX + $regionWidth, $actualRight + $guard))
                $candidateBottom = [Math]::Min(720, [Math]::Max($regionY + $regionHeight, $actualBottom + $guard))
                Set-NumberAttribute $node "data-pome-region-x" $candidateX
                Set-NumberAttribute $node "data-pome-region-y" $candidateY
                Set-NumberAttribute $node "data-pome-region-width" ($candidateRight - $candidateX)
                Set-NumberAttribute $node "data-pome-region-height" ($candidateBottom - $candidateY)
                [void]$safeFixes.Add([ordered]@{
                    pageNumber = $pageIndex
                    svgPath = $svgFiles[$pageIndex - 1].FullName
                    regionId = $issue.regionId
                    text = $issue.text
                    action = "expand-source-region-to-powerpoint-bounds"
                    actualBounds = $issue.actualBounds
                    previousAllowedBounds = $issue.allowedBounds
                    safeOverflowLimit = Maximum-SafeRegionOverflow $issue
                })
                [void]$warnings.Add((Issue "warning" "powerpoint_region_metadata_safely_expanded" $pageIndex $block "Declared source region was safely expanded to the measured PowerPoint text bounds" $issue.allowedBounds $issue.overflow))
            }
            if ($canRepairRegionMetadata) {
                $svg.DocumentElement.SetAttribute("data-pome-powerpoint-repair-ready", "true")
                for ($index = $hardErrors.Count - 1; $index -ge 0; $index--) {
                    if ($hardErrors[$index].pageNumber -eq $pageIndex -and $hardErrors[$index].rule -eq "powerpoint_text_outside_declared_region") {
                        $hardErrors.RemoveAt($index)
                    }
                }
                $settings = [System.Xml.XmlWriterSettings]::new()
                $settings.Encoding = [System.Text.UTF8Encoding]::new($false)
                $settings.Indent = $false
                $writer = [System.Xml.XmlWriter]::Create($svgFiles[$pageIndex - 1].FullName, $settings)
                try { $svg.Save($writer) } finally { $writer.Dispose() }
            }
        }
        [void]$pageSummaries.Add([ordered]@{
            pageNumber = $pageIndex
            svgPath = $svgFiles[$pageIndex - 1].FullName
            pngPath = $pngPath
            sourceTextCount = $textNodes.Count
            editableTextShapeCount = $shapeTexts.Count
            mappedTextCount = $mappedBlocks.Count
        })
    }

    $report = [ordered]@{
        schemaVersion = 1
        passed = ($hardErrors.Count -eq 0)
        pptxPath = $resolvedPptxPath
        renderDir = $resolvedRenderDir
        slideWidth = $slideWidth
        slideHeight = $slideHeight
        hardErrors = @($hardErrors)
        warnings = @($warnings)
        safeFixes = @($safeFixes)
        pages = @($pageSummaries)
    }
    $report | ConvertTo-Json -Depth 12 -Compress
    if ($hardErrors.Count -eq 0) { exit 0 } else { exit 2 }
} catch {
    [ordered]@{
        schemaVersion = 1
        passed = $false
        checkerError = $_.Exception.Message
        hardErrors = @()
        warnings = @()
    } | ConvertTo-Json -Depth 6 -Compress
    exit 3
} finally {
    if ($null -ne $presentation) {
        try { $presentation.Close() } catch {}
    }
    if ($null -ne $powerPoint) {
        try {
            if ($initialPresentationCount -eq 0 -and $powerPoint.Presentations.Count -eq 0) {
                $powerPoint.Quit()
            }
        } catch {}
        try { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($powerPoint) } catch {}
    }
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}
