import Foundation
import AppKit
import SwiftUI
import Charts

struct Point: Identifiable {
    let id: Int
    let depth: Int
    let value: Double
    let series: String
}
struct Panel: View {
    let title: String
    let points: [Point]
    let probability: Bool
    private var lastDepth: Int { points.map(\.depth).max() ?? 0 }
    private var minProbability: Double { pow(10, floor(log10(points.map(\.value).filter{$0>0}.min() ?? 0.000001))) }
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.system(size: 18, weight: .semibold))
            if probability {
                Chart(points) { p in
                    LineMark(x: .value("Depth", p.depth), y: .value("Percent", p.value))
                        .foregroundStyle(by: .value("Configuration", p.series))
                        .lineStyle(StrokeStyle(lineWidth: 2))
                }
                .chartYScale(domain: minProbability...100.0, type: .log)
                .chartYAxis {
                    AxisMarks(values: stride(from: Int(log10(minProbability)), through: 2, by: 2).map{pow(10,Double($0))}) { v in
                        AxisGridLine(); AxisTick()
                        AxisValueLabel { if let x = v.as(Double.self) { Text(String(format: "%g%%", x)) } }
                    }
                }
                .chartXAxis { AxisMarks(values: Array(stride(from: 0, through: lastDepth, by: 2))) }
                .chartXAxisLabel("Join depth d; probability of reaching d (log scale)")
            } else {
                Chart(points) { p in
                    BarMark(x: .value("Depth", String(p.depth)), y: .value("Millions of row visits", p.value))
                        .foregroundStyle(by: .value("Configuration", p.series))
                        .position(by: .value("Configuration", p.series))
                }
                .chartYScale(domain: 0...(max(1,points.map(\.value).max() ?? 1)*1.08))
                .chartXAxisLabel("Join depth after the anchor")
                .chartYAxisLabel("Candidate row visits (millions)")
            }
        }
        .chartForegroundStyleScale(range: [Color(red:0.84,green:0.36,blue:0.12), Color(red:0.05,green:0.39,blue:0.68)])
        .chartLegend(position: .bottom, alignment: .leading)
        .padding(18).background(Color.white).cornerRadius(10)
    }
}
@main struct Plot {
    @MainActor static func main() throws {
        guard CommandLine.arguments.count == 3 else { fatalError("plot RESULTS.json OUTPUT.png") }
        let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let doc = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        let depths = doc["depths"] as! [String: [[String: Any]]]
        var lastDepth: Int = 0
        for rows in depths.values {
            for row in rows where ((row["entries"] as? NSNumber)?.uint64Value ?? 0)>0 {
                lastDepth=max(lastDepth,row["depth"] as? Int ?? 0)
            }
        }
        func points(_ settings: [(String,String)], _ field: String, _ scale: Double, _ skipZero: Bool) -> [Point] {
            var out = [Point]()
            for (key,label) in settings {
                for row in depths[key]! {
                    let d=row["depth"] as! Int
                    if d>lastDepth || (skipZero && d==0) { continue }
                    guard let v=row[field] as? NSNumber else { continue }
                    if field=="reach_probability" && v.doubleValue<=0 {continue}
                    out.append(Point(id:out.count,depth:d,value:v.doubleValue*scale,series:label))
                }
            }
            return out
        }
        let fixed = (doc["depth_contrasts"] as! [String:[String:Any]])["cell_effect_with_refutation"]!["fixed_rule_mix"] as! [[String:Any]]
        var fixedPoints = [Point]()
        for (field,label) in [("before_reach_probability","Cell OFF, fixed mix"),("after_reach_probability","Cell ON, fixed mix")] {
            for row in fixed {
                guard let v=row[field] as? NSNumber, v.doubleValue>0 else {continue}
                fixedPoints.append(Point(id:fixedPoints.count,depth:row["depth"] as! Int,value:v.doubleValue*100,series:label))
            }
        }
        let view = VStack(alignment:.leading,spacing:16) {
            Text("Join search depth | raytrace Pass 3").font(.system(size:30,weight:.bold))
            Text("CPU Cell services = certified saturation + complete query reuse. Refutation = negative lookahead + key fingerprints.")
                .font(.system(size:14)).foregroundStyle(.secondary)
            HStack(spacing:16) {
                Panel(title:"A. Cell ON: refutation saves shallow and deep work",points:points([("s1r0","Refutation OFF"),("s1r1","Refutation ON")],"live_rows",0.000001,true),probability:false)
                Panel(title:"B. Cell ON: probability of reaching depth d",points:points([("s1r0","Refutation OFF"),("s1r1","Refutation ON")],"reach_probability",100,false),probability:true)
            }.frame(height:390)
            HStack(spacing:16) {
                Panel(title:"C. Refutation ON: Cell reduces the GPU workload",points:points([("s0r1","Cell OFF, refutation ON"),("s1r1","Cell ON, refutation ON")],"live_rows",0.000001,true),probability:false)
                Panel(title:"D. Refutation ON, fixed rule mix: curves coincide",points:fixedPoints,probability:true)
            }.frame(height:390)
            Text("Depth 0 = validated multi-atom anchor. Lines show Pr(maximum attempted depth >= d), not loop-entry counts. Counts include retries.\nFour same-commit state checks passed. Zero probabilities and unobserved trailing depths are omitted from plots. Instrumented time is not a speedup.")
                .font(.system(size:12)).foregroundStyle(.secondary)
        }.padding(24).frame(width:1440,height:980).background(Color(red:0.95,green:0.96,blue:0.97)).environment(\.colorScheme,.light)
        let renderer = ImageRenderer(content:view)
        renderer.proposedSize=ProposedViewSize(width:1440,height:980);renderer.scale=1
        guard let image=renderer.nsImage, let tiff=image.tiffRepresentation,
              let rep=NSBitmapImageRep(data:tiff),let png=rep.representation(using:.png,properties:[:]) else {fatalError("chart rendering failed")}
        try png.write(to:URL(fileURLWithPath:CommandLine.arguments[2]))
    }
}
