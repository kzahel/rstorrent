import Ajv2020, { type ValidateFunction } from "ajv/dist/2020";

import schema from "./generated/v1.schema.json";

const SCHEMA_ID = "https://rstorrent.invalid/schemas/api/v1";
const ajv = new Ajv2020({
  allErrors: true,
  strict: true,
  validateFormats: false,
});
ajv.addSchema(schema, SCHEMA_ID);

const validators = new Map<string, ValidateFunction>();

export class SchemaError extends Error {}

export function assertApiSchema<T>(
  definition: string,
  value: unknown,
): asserts value is T {
  let validator = validators.get(definition);
  if (validator === undefined) {
    validator = ajv.getSchema(`${SCHEMA_ID}#/$defs/${definition}`);
    if (validator === undefined) {
      throw new SchemaError(`generated schema ${definition} is unavailable`);
    }
    validators.set(definition, validator);
  }
  if (!validator(value)) {
    const detail = ajv.errorsText(validator.errors, { separator: "; " });
    throw new SchemaError(`${definition} failed schema validation: ${detail}`);
  }
}
