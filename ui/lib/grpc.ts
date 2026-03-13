// gRPC client utilities for calling Rust servers

import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";
import path from "path";

const PROTO_DIR =
  process.env.PROTO_DIR || path.resolve(process.cwd(), "..", "proto");

function loadProto(filename: string) {
  const pkgDef = protoLoader.loadSync(path.join(PROTO_DIR, filename), {
    keepCase: true,
    longs: Number,
    enums: String,
    defaults: true,
    oneofs: true,
  });
  return grpc.loadPackageDefinition(pkgDef);
}

// Lazy-loaded clients
let registryClient: any = null;
const merchantClients: Map<string, any> = new Map();

export function getRegistryClient(
  address = process.env.REGISTRY_ADDRESS || "[::1]:50051"
) {
  if (!registryClient) {
    const proto = loadProto("registry.proto") as any;
    registryClient = new proto.registry.Registry(
      address,
      grpc.credentials.createInsecure()
    );
  }
  return registryClient;
}

export function getMerchantClient(address: string) {
  let client = merchantClients.get(address);
  if (!client) {
    const proto = loadProto("merchant.proto") as any;
    client = new proto.merchant.MerchantService(
      address,
      grpc.credentials.createInsecure()
    );
    merchantClients.set(address, client);
  }
  return client;
}

// Promisify gRPC unary calls
export function grpcCall<T>(
  client: any,
  method: string,
  request: any
): Promise<T> {
  return new Promise((resolve, reject) => {
    client[method](request, (err: any, response: T) => {
      if (err) reject(err);
      else resolve(response);
    });
  });
}

export function resetClients(): void {
  if (registryClient) {
    registryClient.close();
    registryClient = null;
  }
  for (const client of merchantClients.values()) {
    client.close();
  }
  merchantClients.clear();
}
