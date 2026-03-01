// param_type: Vec<String>
// default: vec![]
// enable_if: "!param.is_empty()"
// enable_value: "vec![\"steel-axe\".to_string()]"
const requiredResearch: string[] = PARAM_VALUE as any

afterReplay(() => {
  const force = game.forces["player"]
  for (const name of requiredResearch) {
    const tech = force.technologies[name]
    if (!tech) {
      ReplayLog.err(`Required research "${name}" not found`)
    } else if (!tech.researched) {
      ReplayLog.err(`Required research "${name}" not completed`)
    } else {
      ReplayLog.info(`Required research "${name}" completed`)
    }
  }
})
