#include "persistence/EditStateJson.h"

#include <QJsonArray>

#include <cmath>

namespace iris {

namespace {

// Mask coordinates are stored with 5 decimals (well below a pixel), keeping sidecars small.
double rounded(float v)
{
    return std::round(double(v) * 1e5) / 1e5;
}

float number(const QJsonObject& json, const char* key, float fallback)
{
    const QJsonValue value = json.value(QLatin1String(key));
    return value.isDouble() ? float(value.toDouble()) : fallback;
}

} // namespace

QJsonObject adjustmentsToJson(const BasicAdjustments& adjustments)
{
    BasicAdjustments copy = adjustments;
    QJsonObject json;
    for (const AdjustmentField& field : adjustmentFields())
        json.insert(QLatin1String(field.key), double(field.value(copy)));
    return json;
}

void readAdjustments(const QJsonObject& json, BasicAdjustments& adjustments)
{
    for (const AdjustmentField& field : adjustmentFields()) {
        const QJsonValue value = json.value(QLatin1String(field.key));
        if (value.isDouble())
            field.value(adjustments) = float(value.toDouble());
    }
    adjustments = sanitized(adjustments);
}

QJsonObject toneCurveToJson(const ToneCurve& curve)
{
    QJsonArray points;
    for (const CurvePoint& p : curve.rgb)
        points.append(QJsonArray{double(p.x), double(p.y)});
    return QJsonObject{{"points", points}};
}

std::optional<ToneCurve> readToneCurve(const QJsonValue& json)
{
    const QJsonValue points = json.toObject().value("points");
    if (!points.isArray())
        return std::nullopt;
    CurvePoints parsed;
    for (const QJsonValue& value : points.toArray()) {
        const QJsonArray pair = value.toArray();
        if (pair.size() != 2 || !pair[0].isDouble() || !pair[1].isDouble())
            return std::nullopt;
        parsed.push_back({float(pair[0].toDouble()), float(pair[1].toDouble())});
    }
    ToneCurve curve;
    curve.rgb = normalizedCurve(parsed);
    return curve;
}

QJsonObject hslToJson(const HslAdjustments& hsl)
{
    QJsonObject json;
    for (int i = 0; i < kHslColorCount; ++i) {
        const HslBand& band = hsl.bands[i];
        if (band == HslBand{})
            continue;
        json.insert(QLatin1String(hslColorKey(HslColor(i))),
                    QJsonObject{{"hue", double(band.hue)},
                                {"saturation", double(band.saturation)},
                                {"luminance", double(band.luminance)}});
    }
    return json;
}

HslAdjustments readHsl(const QJsonValue& json)
{
    HslAdjustments hsl;
    const QJsonObject object = json.toObject();
    for (int i = 0; i < kHslColorCount; ++i) {
        const QJsonObject band = object.value(QLatin1String(hslColorKey(HslColor(i)))).toObject();
        hsl.bands[i].hue = float(band.value("hue").toDouble(0));
        hsl.bands[i].saturation = float(band.value("saturation").toDouble(0));
        hsl.bands[i].luminance = float(band.value("luminance").toDouble(0));
    }
    return sanitized(hsl);
}

QJsonArray masksToJson(const std::vector<Mask>& masks)
{
    QJsonArray array;
    for (const Mask& mask : masks) {
        QJsonObject json;
        json.insert("type", maskTypeKey(mask.type));
        json.insert("name", QString::fromStdString(mask.name));
        json.insert("invert", mask.invert);
        if (mask.type == MaskType::Linear) {
            const LinearGradient& l = mask.linear;
            json.insert("linear", QJsonObject{{"x", rounded(l.x)},
                                              {"y", rounded(l.y)},
                                              {"angle", rounded(l.angle)},
                                              {"feather", rounded(l.feather)}});
        } else if (mask.type == MaskType::Radial) {
            const RadialGradient& r = mask.radial;
            json.insert("radial", QJsonObject{{"x", rounded(r.x)},
                                              {"y", rounded(r.y)},
                                              {"width", rounded(r.width)},
                                              {"height", rounded(r.height)},
                                              {"rotation", rounded(r.rotation)},
                                              {"feather", rounded(r.feather)}});
        }
        QJsonArray strokes;
        for (const BrushStroke& stroke : mask.strokes) {
            QJsonArray points;
            for (const MaskPoint& p : stroke.points)
                points.append(QJsonArray{rounded(p.x), rounded(p.y)});
            strokes.append(QJsonObject{{"mode", brushModeKey(stroke.mode)},
                                       {"radius", rounded(stroke.radius)},
                                       {"feather", rounded(stroke.feather)},
                                       {"opacity", rounded(stroke.opacity)},
                                       {"points", points}});
        }
        json.insert("strokes", strokes);
        QJsonObject adjustments;
        LocalAdjustments copy = mask.adjustments;
        for (const LocalAdjustmentField& field : localAdjustmentFields())
            if (const float v = field.value(copy); v != 0)
                adjustments.insert(QLatin1String(field.key), double(v));
        json.insert("adjustments", adjustments);
        array.append(json);
    }
    return array;
}

std::vector<Mask> readMasks(const QJsonValue& json)
{
    std::vector<Mask> masks;
    for (const QJsonValue& value : json.toArray()) {
        const QJsonObject object = value.toObject();
        const QString type = object.value("type").toString();
        Mask mask;
        if (type == maskTypeKey(MaskType::Brush))
            mask.type = MaskType::Brush;
        else if (type == maskTypeKey(MaskType::Linear))
            mask.type = MaskType::Linear;
        else if (type == maskTypeKey(MaskType::Radial))
            mask.type = MaskType::Radial;
        else
            continue;
        mask.name = object.value("name").toString().toStdString();
        if (mask.name.empty())
            mask.name = newMask(mask.type, masks).name;
        mask.invert = object.value("invert").toBool(false);

        const QJsonObject l = object.value("linear").toObject();
        mask.linear = {number(l, "x", mask.linear.x), number(l, "y", mask.linear.y),
                       number(l, "angle", mask.linear.angle), number(l, "feather", mask.linear.feather)};
        const QJsonObject r = object.value("radial").toObject();
        mask.radial = {number(r, "x", mask.radial.x),          number(r, "y", mask.radial.y),
                       number(r, "width", mask.radial.width),  number(r, "height", mask.radial.height),
                       number(r, "rotation", mask.radial.rotation), number(r, "feather", mask.radial.feather)};

        for (const QJsonValue& strokeValue : object.value("strokes").toArray()) {
            const QJsonObject s = strokeValue.toObject();
            BrushStroke stroke;
            const QString mode = s.value("mode").toString();
            stroke.mode = mode == brushModeKey(BrushMode::Subtract) ? BrushMode::Subtract
                          : mode == brushModeKey(BrushMode::Erase)  ? BrushMode::Erase
                                                                    : BrushMode::Add;
            stroke.radius = number(s, "radius", stroke.radius);
            stroke.feather = number(s, "feather", stroke.feather);
            stroke.opacity = number(s, "opacity", stroke.opacity);
            for (const QJsonValue& pointValue : s.value("points").toArray()) {
                const QJsonArray pair = pointValue.toArray();
                if (pair.size() == 2 && pair[0].isDouble() && pair[1].isDouble())
                    stroke.points.push_back({float(pair[0].toDouble()), float(pair[1].toDouble())});
            }
            mask.strokes.push_back(std::move(stroke));
        }

        const QJsonObject adjustments = object.value("adjustments").toObject();
        for (const LocalAdjustmentField& field : localAdjustmentFields())
            field.value(mask.adjustments) = number(adjustments, field.key, 0);

        masks.push_back(sanitized(std::move(mask)));
        if (masks.size() == kMaxMasks)
            break;
    }
    return masks;
}

} // namespace iris
